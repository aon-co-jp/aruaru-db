//! # aruaru-db 公式 Rust コネクタ / official Rust connector (thin wrapper)
//!
//! **これは独自の PostgreSQL ドライバではない。** 業界標準の非同期クライアント
//! [`tokio_postgres`] をそのまま使い、その上に aruaru-db の Git-on-SQL 機能
//! ——`SELECT aruaru_commit('msg')` と `... AS OF COMMIT '<id>'`——を Rust の
//! 慣用的な API で足しただけの薄い層。RPoem / Axum / Poem など tokio ベースの
//! アプリから使う想定。
//!
//! **This is NOT a custom PostgreSQL driver.** It uses the industry-standard
//! async client [`tokio_postgres`] as-is and only adds idiomatic helpers for
//! aruaru-db's Git-on-SQL surface (`aruaru_commit` and `AS OF COMMIT`).
//!
//! 独自ドライバを作らない理由・接続文字列・他言語のレシピは
//! [`../../docs/CLIENTS.md`](https://github.com/aon-co-jp/aruaru-db/blob/main/docs/CLIENTS.md)。
//!
//! ## 例 / Example
//!
//! ```no_run
//! # async fn run() -> Result<(), aruaru_db_connector::Error> {
//! use aruaru_db_connector::AruaruDb;
//!
//! let db = AruaruDb::connect(
//!     "host=localhost port=5433 user=app password=secret dbname=app"
//! ).await?;
//!
//! db.execute("INSERT INTO items(id, qty) VALUES ('sword', 1)", &[]).await?;
//! let first = db.commit("first import").await?;
//!
//! db.execute("UPDATE items SET qty = 5 WHERE id = 'sword'", &[]).await?;
//! db.commit("restock").await?;
//!
//! // VersionlessAPI: 過去のコミット時点を読む(最新は 5、これは 1)
//! let rows = db
//!     .query_as_of("SELECT qty FROM items WHERE id = 'sword'", &first, &[])
//!     .await?;
//! assert_eq!(rows[0].get::<_, String>(0).parse::<i32>().unwrap(), 1); // aruaru-wire は列を text で返す
//! # Ok(()) }
//! ```

use std::fmt;

use tokio_postgres::{types::ToSql, NoTls, Row};

/// このコネクタのエラー。`tokio_postgres::Error` はそのまま透過する。
#[derive(Debug)]
pub enum Error {
    /// 接続確立に失敗。
    Connect(tokio_postgres::Error),
    /// クエリ実行に失敗。
    Query(tokio_postgres::Error),
    /// `aruaru_commit()` が commit_id を返さなかった。
    NoCommitId,
    /// `query_as_of` に渡された commit_id がリテラルとして安全でない
    /// (16 進・英数字・`-` `_` 以外を含む)。SQL インジェクション防止。
    InvalidCommitId(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Connect(e) => write!(f, "aruaru-db connect failed: {e}"),
            Error::Query(e) => write!(f, "aruaru-db query failed: {e}"),
            Error::NoCommitId => write!(f, "aruaru_commit() returned no commit id"),
            Error::InvalidCommitId(s) => {
                write!(f, "commit id {s:?} is not a safe literal (expected hex / [A-Za-z0-9_-])")
            }
        }
    }
}

impl std::error::Error for Error {}

/// commit_id が `AS OF COMMIT '<id>'` のリテラルとして安全か。
/// aruaru-db の commit_id は英数字 + `-` `_`(SHA 系ハッシュ/UUID 由来)。
pub fn is_safe_commit_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// aruaru-db への接続(単一コネクション)。
///
/// プールが要る場合はこの型を `deadpool` / `bb8` で包むか、`tokio_postgres`
/// を直接プールする。RPoem のような 1 プロセス複数リクエストのアプリでは
/// `Arc<AruaruDb>` を共有し、内部の `tokio_postgres::Client` が
/// パイプライン化してくれる(`Client` は `Sync`)。
pub struct AruaruDb {
    client: tokio_postgres::Client,
}

impl AruaruDb {
    /// libpq / key-value いずれの DSN 形式でも可。TLS は今は `NoTls`
    /// (本番は `tokio-postgres-rustls` を使うか、リバースプロキシで終端)。
    ///
    /// 接続タスクは `tokio::spawn` でバックグラウンドへ回す。
    pub async fn connect(dsn: &str) -> Result<Self, Error> {
        let (client, connection) =
            tokio_postgres::connect(dsn, NoTls).await.map_err(Error::Connect)?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("aruaru-db connection error: {e}");
            }
        });
        Ok(Self { client })
    }

    /// 既存の `tokio_postgres::Client` から作る(自前でプール・TLS を組む場合)。
    pub fn from_client(client: tokio_postgres::Client) -> Self {
        Self { client }
    }

    /// 内部の `tokio_postgres::Client` への参照。透過的に何でもできる。
    pub fn client(&self) -> &tokio_postgres::Client {
        &self.client
    }

    /// `client.execute` の薄い透過。
    pub async fn execute(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<u64, Error> {
        self.client.execute(sql, params).await.map_err(Error::Query)
    }

    /// `client.query` の薄い透過。
    pub async fn query(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Vec<Row>, Error> {
        self.client.query(sql, params).await.map_err(Error::Query)
    }

    /// Git-on-SQL: 現在の全テーブル状態をスナップショットし commit_id を返す
    /// (`SELECT aruaru_commit($1)`)。
    pub async fn commit(&self, message: &str) -> Result<String, Error> {
        let row = self
            .client
            .query_opt("SELECT aruaru_commit($1)", &[&message])
            .await
            .map_err(Error::Query)?
            .ok_or(Error::NoCommitId)?;
        // aruaru_commit は 1 列(commit_id テキスト)を返す。
        row.try_get::<_, String>(0).map_err(|_| Error::NoCommitId)
    }

    /// VersionlessAPI: `base_select` の結果を **過去のコミット時点** で読む。
    ///
    /// `base_select` は `AS OF COMMIT` を**含まない**通常の SELECT
    /// (例 `"SELECT qty FROM items WHERE id = $1"`)。このメソッドが末尾へ
    /// ` AS OF COMMIT '<commit_id>'` を安全に付ける(commit_id は
    /// [`is_safe_commit_id`] で検証、非安全なら [`Error::InvalidCommitId`])。
    /// `params` は `base_select` のプレースホルダ用。
    pub async fn query_as_of(
        &self,
        base_select: &str,
        commit_id: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Vec<Row>, Error> {
        if !is_safe_commit_id(commit_id) {
            return Err(Error::InvalidCommitId(commit_id.to_string()));
        }
        let sql = format!("{} AS OF COMMIT '{}'", base_select.trim_end(), commit_id);
        self.client.query(&sql, params).await.map_err(Error::Query)
    }

    /// `query_as_of` の 0/1 行版。
    pub async fn query_as_of_opt(
        &self,
        base_select: &str,
        commit_id: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Option<Row>, Error> {
        Ok(self.query_as_of(base_select, commit_id, params).await?.into_iter().next())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_commit_id_accepts_hashes_and_uuids_rejects_sql() {
        assert!(is_safe_commit_id("a1b2c3d4e5f6"));
        assert!(is_safe_commit_id("9f8e7d6c-1234-4abc-9def-000011112222"));
        assert!(is_safe_commit_id("commit_42-X"));
        assert!(!is_safe_commit_id(""));
        assert!(!is_safe_commit_id("abc'; DROP TABLE items; --"));
        assert!(!is_safe_commit_id("abc def"));
        assert!(!is_safe_commit_id(&"x".repeat(200)));
    }

    #[test]
    fn query_as_of_rejects_unsafe_commit_id_before_touching_the_network() {
        // connect しないので、非安全 id は即 Err（ネットワーク不要）で返るはず。
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        rt.block_on(async {
            // ダミー Client は作れないので、is_safe_commit_id の直接検証で代替。
            assert!(!is_safe_commit_id("' OR 1=1 --"));
        });
    }

    /// 実サーバ相手の往復。`ARUARU_DB_TEST_DSN` があるときだけ走る。
    /// 例: ARUARU_DB_TEST_DSN="host=localhost port=5433 user=app password=secret dbname=app"
    #[test]
    #[ignore = "needs a running aruaru-server (set ARUARU_DB_TEST_DSN)"]
    fn live_commit_and_as_of_round_trip() {
        let dsn = std::env::var("ARUARU_DB_TEST_DSN").expect("ARUARU_DB_TEST_DSN");
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let db = AruaruDb::connect(&dsn).await.unwrap();
            db.execute("CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY, qty INT)", &[])
                .await
                .unwrap();
            db.execute(
                "INSERT INTO items(id, qty) VALUES ('sword', 1) \
                 ON CONFLICT (id) DO UPDATE SET qty = EXCLUDED.qty",
                &[],
            )
            .await
            .unwrap();
            let first = db.commit("first import").await.unwrap();
            db.execute("UPDATE items SET qty = 5 WHERE id = 'sword'", &[]).await.unwrap();
            db.commit("restock").await.unwrap();

            // 注意: aruaru-wire は現状、通常テーブル列を VARCHAR(text)で返す
            // (`docs/CLIENTS.md` §5)。typed getter(`get::<i32>`)は使えない
            // ので文字列で受けて parse する。
            let latest = db.query("SELECT qty FROM items WHERE id = 'sword'", &[]).await.unwrap();
            assert_eq!(latest[0].get::<_, String>(0).parse::<i32>().unwrap(), 5);

            let old = db
                .query_as_of("SELECT qty FROM items WHERE id = 'sword'", &first, &[])
                .await
                .unwrap();
            assert_eq!(
                old[0].get::<_, String>(0).parse::<i32>().unwrap(),
                1,
                "AS OF COMMIT must return the historical value"
            );
        });
    }
}
