//! DUAL DATABASE構成(aruaru-db × PostgreSQL、`open-web-server/CLAUDE.md`
//! 拡張要件(4)「DATABASE書き込みの四重化」の**②aruaru-db**を、実際に
//! PostgreSQLへ同期ミラーする第一実装。
//!
//! ## 位置づけ・既存実装との関係
//!
//! - **①PostgreSQL**(`open-web-server-ledger::postgres_wal::PostgresWal`)・
//!   **③マルチリージョン同期レプリケーション**(同`multi_region`)・
//!   **④独立監査ログ**(同`audit_log`)は`open-web-server`リポジトリ側に
//!   自己完結して実装済み。本モジュールはそれらと対になる**②**を
//!   `aruaru-db`側の責務として実装する — aruaru-dbの`VersionController`
//!   (Git-on-SQL、`Commit`/`CommitId`)がVersionlessAPIの「バージョン管理・
//!   Git管理」を担う一方、外部の実PostgreSQLへも同一ミューテーションを
//!   同期的にミラーすることで、aruaru-db自体が単一障害点になることを防ぐ。
//! - `aruaru-migrate::target::TargetClient`(tokio-postgres経由でaruaru-dbへ
//!   書き込む、既存実装)とは向きが逆: あちらは「外部DB → aruaru-db」の
//!   一括移行ツール、本モジュールは「aruaru-dbのコミット → 外部PostgreSQL」
//!   の**都度**同期ミラーであり、ライブトラフィックのホットパスで使う。
//!
//! ## 設計(`open-web-server-ledger`の既存パターンを踏襲)
//!
//! - **同期**: `mirror()`はPostgreSQLへのINSERTが完了する(またはエラーに
//!   なる)までブロックする。fire-and-forgetにしない — 金融データに
//!   eventual consistencyは許されないという`multi_region.rs`と同じ判断。
//! - **冪等性**: `idempotency_key`に一意制約を張り、`INSERT ... ON CONFLICT
//!   DO NOTHING`で再送に強くする(`postgres_wal.rs`と同じ形状)。
//! - **VersionlessAPI対応**: 行は`(target, key)`の最新版だけでなく、
//!   `commit_id`列で当該ミューテーションが属するaruaru-dbコミットに
//!   タグ付けする。これにより`WHERE key = $1 ORDER BY committed_at DESC
//!   LIMIT 1`で「バージョンレス」な最新値取得、`WHERE commit_id = $2`で
//!   「Git版管理」的な特定コミット時点の値取得の両方を、PostgreSQL側
//!   だけからでも行える(aruaru-db側の`SELECT ... AS OF COMMIT`と同じ
//!   意味論を、ミラー先のPostgreSQLでも再現する)。
//! - **失敗ポリシー**: aruaru-db側のコミット自体はこのモジュールの責務
//!   外(呼び出し側が`VersionController::commit`等で先に確定させる)。
//!   `mirror()`が失敗した場合、aruaru-db側のコミットをロールバックする
//!   手段は無い(2フェーズコミットではない、`multi_region.rs`と同じ
//!   スコープの限界)。`DualDatabaseError`に失敗を保持して返し、
//!   呼び出し側が独立監査ログ(④)へ記録する・アラートを上げる等の
//!   対応判断に使えるようにする。同一`idempotency_key`での再送は
//!   `ON CONFLICT DO NOTHING`により安全に再試行できる。
//!
//! ## 正直な開示(このパスのスコープ外)
//!
//! - **実PostgreSQL接続での検証は未実施**(この開発環境に到達可能な
//!   PostgreSQLインスタンスが無いため、`postgres_wal.rs`と同じ制約)。
//!   SQL文字列自体の単体テストと、`DATABASE_URL`環境変数がある場合のみ
//!   実行される`#[ignore]`統合テストの2段構えで検証可能性を確保した。
//! - **2フェーズコミット・分散トランザクションではない**: aruaru-db側の
//!   コミットとPostgreSQL側のINSERTは独立した操作であり、片方だけが
//!   成功する状態が発生し得る(上記の失敗ポリシー参照)。真のXA/
//!   2PCサポートは将来の拡張。

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::fmt;

/// 最小スキーマ。`postgres_wal.rs::SCHEMA_SQL`と同じ形状(idempotency_keyに
/// 一意制約)に、VersionlessAPI用の`commit_id`/`table_name`列を加えたもの。
pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS aruaru_dual_mirror (
    idempotency_key TEXT PRIMARY KEY,
    table_name      TEXT NOT NULL,
    row_key         TEXT NOT NULL,
    payload         JSONB NOT NULL,
    commit_id       TEXT NOT NULL,
    committed_at    TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS aruaru_dual_mirror_latest_idx
    ON aruaru_dual_mirror (table_name, row_key, committed_at DESC);
CREATE INDEX IF NOT EXISTS aruaru_dual_mirror_commit_idx
    ON aruaru_dual_mirror (commit_id);
"#;

/// aruaru-db側で既に確定した1ミューテーション(呼び出し側が
/// `VersionController::commit`等で先にコミット済みであることが前提)。
#[derive(Debug, Clone)]
pub struct MirroredMutation {
    pub table_name: String,
    pub row_key: String,
    pub payload_json: String,
    pub commit_id: String,
    pub committed_at: DateTime<Utc>,
}

impl MirroredMutation {
    /// `(table_name, row_key, commit_id)`から冪等性キーを導出する。同一
    /// コミット内で同一行への複数回の呼び出し(通常は起こらないが、
    /// リトライによる再送は起こり得る)を安全にデデュープする。
    pub fn idempotency_key(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.table_name.as_bytes());
        hasher.update(b"\0");
        hasher.update(self.row_key.as_bytes());
        hasher.update(b"\0");
        hasher.update(self.commit_id.as_bytes());
        hex::encode(hasher.finalize())
    }
}

#[derive(Debug)]
pub enum DualDatabaseError {
    Sqlx(sqlx::Error),
}

impl fmt::Display for DualDatabaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlx(e) => write!(f, "dual-database mirror failed: {e}"),
        }
    }
}

impl std::error::Error for DualDatabaseError {}

impl From<sqlx::Error> for DualDatabaseError {
    fn from(e: sqlx::Error) -> Self {
        Self::Sqlx(e)
    }
}

/// aruaru-dbのコミット済みミューテーションを、実PostgreSQLへ同期ミラーする。
pub struct DualDatabaseMirror {
    pool: PgPool,
}

impl DualDatabaseMirror {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// スキーマを準備する(`CREATE TABLE IF NOT EXISTS`、複数回呼んでも安全)。
    pub async fn ensure_schema(&self) -> Result<(), DualDatabaseError> {
        sqlx::raw_sql(SCHEMA_SQL).execute(&self.pool).await?;
        Ok(())
    }

    /// 1ミューテーションをPostgreSQLへ同期的にミラーする。呼び出しは
    /// INSERTが完了する(コンフリクトでの黙殺も含め)までブロックする。
    pub async fn mirror(&self, mutation: &MirroredMutation) -> Result<(), DualDatabaseError> {
        let idempotency_key = mutation.idempotency_key();
        sqlx::query(
            r#"
            INSERT INTO aruaru_dual_mirror
                (idempotency_key, table_name, row_key, payload, commit_id, committed_at)
            VALUES ($1, $2, $3, $4::jsonb, $5, $6)
            ON CONFLICT (idempotency_key) DO NOTHING
            "#,
        )
        .bind(&idempotency_key)
        .bind(&mutation.table_name)
        .bind(&mutation.row_key)
        .bind(&mutation.payload_json)
        .bind(&mutation.commit_id)
        .bind(mutation.committed_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// VersionlessAPI: 指定行の「最新版」をPostgreSQL側から取得する
    /// (`committed_at`降順で1件、aruaru-db側に問い合わせずに済む読み出し
    /// フォールバック用途)。
    pub async fn latest(&self, table_name: &str, row_key: &str) -> Result<Option<String>, DualDatabaseError> {
        let row: Option<(String,)> = sqlx::query_as(
            r#"
            SELECT payload::text FROM aruaru_dual_mirror
            WHERE table_name = $1 AND row_key = $2
            ORDER BY committed_at DESC
            LIMIT 1
            "#,
        )
        .bind(table_name)
        .bind(row_key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(payload,)| payload))
    }

    /// Git版管理: 指定`commit_id`に属する当該行の値を取得する(aruaru-db側の
    /// `SELECT ... AS OF COMMIT`と同じ意味論をPostgreSQL側でも再現する)。
    pub async fn at_commit(
        &self,
        table_name: &str,
        row_key: &str,
        commit_id: &str,
    ) -> Result<Option<String>, DualDatabaseError> {
        let row: Option<(String,)> = sqlx::query_as(
            r#"
            SELECT payload::text FROM aruaru_dual_mirror
            WHERE table_name = $1 AND row_key = $2 AND commit_id = $3
            "#,
        )
        .bind(table_name)
        .bind(row_key)
        .bind(commit_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(payload,)| payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_sql_creates_expected_table_and_indexes() {
        assert!(SCHEMA_SQL.contains("CREATE TABLE IF NOT EXISTS aruaru_dual_mirror"));
        assert!(SCHEMA_SQL.contains("idempotency_key TEXT PRIMARY KEY"));
        assert!(SCHEMA_SQL.contains("aruaru_dual_mirror_latest_idx"));
        assert!(SCHEMA_SQL.contains("aruaru_dual_mirror_commit_idx"));
    }

    fn sample_mutation() -> MirroredMutation {
        MirroredMutation {
            table_name: "items".to_string(),
            row_key: "sword".to_string(),
            payload_json: r#"{"qty":1}"#.to_string(),
            commit_id: "abc123".to_string(),
            committed_at: Utc::now(),
        }
    }

    #[test]
    fn idempotency_key_is_deterministic_for_same_inputs() {
        let a = sample_mutation();
        let b = sample_mutation();
        assert_eq!(a.idempotency_key(), b.idempotency_key());
    }

    #[test]
    fn idempotency_key_differs_when_commit_id_differs() {
        let a = sample_mutation();
        let mut b = sample_mutation();
        b.commit_id = "def456".to_string();
        assert_ne!(a.idempotency_key(), b.idempotency_key());
    }

    #[test]
    fn idempotency_key_differs_when_row_key_differs() {
        let a = sample_mutation();
        let mut b = sample_mutation();
        b.row_key = "shield".to_string();
        assert_ne!(a.idempotency_key(), b.idempotency_key());
    }

    #[test]
    fn idempotency_key_differs_when_table_name_differs() {
        let a = sample_mutation();
        let mut b = sample_mutation();
        b.table_name = "inventory".to_string();
        assert_ne!(a.idempotency_key(), b.idempotency_key());
    }

    #[test]
    fn idempotency_key_is_64_char_hex_sha256() {
        let key = sample_mutation().idempotency_key();
        assert_eq!(key.len(), 64);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // 実PostgreSQLに対する統合テスト。`DATABASE_URL`環境変数が設定されて
    // いる場合のみ `cargo test -- --ignored` で実行される
    // (`postgres_wal.rs`と同じ2段構えの検証方針)。
    #[tokio::test]
    #[ignore]
    async fn mirror_then_latest_and_at_commit_round_trip_against_real_postgres() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for this ignored test");
        let pool = PgPool::connect(&url).await.expect("connect to real postgres");
        let mirror = DualDatabaseMirror::new(pool);
        mirror.ensure_schema().await.expect("ensure_schema");

        let m1 = MirroredMutation {
            table_name: "items".to_string(),
            row_key: "sword".to_string(),
            payload_json: r#"{"qty":1}"#.to_string(),
            commit_id: "commit-1".to_string(),
            committed_at: Utc::now(),
        };
        mirror.mirror(&m1).await.expect("mirror m1");

        let m2 = MirroredMutation {
            table_name: "items".to_string(),
            row_key: "sword".to_string(),
            payload_json: r#"{"qty":5}"#.to_string(),
            commit_id: "commit-2".to_string(),
            committed_at: Utc::now(),
        };
        mirror.mirror(&m2).await.expect("mirror m2");

        // VersionlessAPI: 最新は commit-2 の qty=5
        let latest = mirror.latest("items", "sword").await.expect("latest").expect("row exists");
        assert!(latest.contains("\"qty\":5") || latest.contains("\"qty\": 5"));

        // Git版管理: commit-1 を指定すれば qty=1 が返る(最新に上書きされない)
        let at_commit1 = mirror.at_commit("items", "sword", "commit-1").await.expect("at_commit").expect("row exists");
        assert!(at_commit1.contains("\"qty\":1") || at_commit1.contains("\"qty\": 1"));

        // 冪等性: 同一ミューテーションを再送してもエラーにならず、重複行も増えない
        mirror.mirror(&m1).await.expect("re-mirror m1 is idempotent");
    }
}
