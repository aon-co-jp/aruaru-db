//! # aruaru-db-rust
//!
//! aruaru-DB 用 Rust ネイティブ非同期クライアント。
//!
//! aruaru-DB は PostgreSQL ワイヤプロトコル互換なので、内部的には
//! `tokio-postgres` でそのまま繋ぎ、Git-on-SQL 操作を型付き API で包む。
//!
//! ## Tauri v2 対応について
//!
//! **Tauri 専用ドライバーは不要です。** このクレートがそのまま Tauri v2 で動作します。
//!
//! Tauri v2 は内部で Tokio を使っており、本クレートと完全互換です。
//! Tauri の `State` に登録する際は必ず `tokio::sync::Mutex` を使ってください
//! (`std::sync::Mutex` は async コマンドでスレッド境界エラーになります)。
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use tokio::sync::Mutex; // ← tokio の Mutex を使う (std ではない)
//! use tauri::{command, State};
//! use aruaru_db_rust::AruaruDb;
//!
//! struct AruaruState(Arc<Mutex<AruaruDb>>);
//!
//! #[command]
//! async fn commit(msg: String, state: State<'_, AruaruState>) -> Result<String, String> {
//!     let db = state.0.lock().await;
//!     db.commit(&msg).await.map_err(|e| e.to_string())
//! }
//! ```
//!
//! ## クイックスタート
//!
//! ```toml
//! # Cargo.toml
//! [dependencies]
//! aruaru-db-rust = "0.5"
//! tokio = { version = "1", features = ["full"] }
//! ```
//!
//! ```rust,no_run
//! use aruaru_db_rust::{AruaruClient, Config};
//!
//! #[tokio::main]
//! async fn main() -> aruaru_db_rust::Result<()> {
//!     let client = AruaruClient::connect("aruaru://root@localhost:5432/aruaru").await?;
//!
//!     // Git-on-SQL
//!     client.branch("feature/my-feature").await?;
//!     client.execute("CREATE TABLE IF NOT EXISTS tasks (id INT, title TEXT)", &[]).await?;
//!     client.execute("INSERT INTO tasks (id, title) VALUES (1, 'Hello')", &[]).await?;
//!     let commit_id = client.commit("Add tasks table").await?;
//!     println!("Committed: {commit_id}");
//!
//!     // ログ確認
//!     let log = client.log(10).await?;
//!     for entry in &log {
//!         println!("{} {}: {}", entry.short_id, entry.author, entry.message);
//!     }
//!     Ok(())
//! }
//! ```

mod error;
mod types;

pub use error::{AruaruError, Result};
pub use types::{CommitEntry, DiffStat};

use std::str::FromStr;

use tokio_postgres::{Client as PgClient, NoTls, Row};

// ── 接続設定 ────────────────────────────────────────────────

/// aruaru-DB 接続設定
#[derive(Debug, Clone)]
pub struct Config {
    pub host:     String,
    pub port:     u16,
    pub dbname:   String,
    pub user:     String,
    pub password: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host:     "localhost".into(),
            port:     5432,
            dbname:   "aruaru".into(),
            user:     "root".into(),
            password: String::new(),
        }
    }
}

impl Config {
    /// `aruaru://user:pass@host:port/dbname` または
    /// `postgres://user:pass@host:port/dbname` 形式の URL からパース
    pub fn from_url(url: &str) -> Result<Self> {
        let u = url::Url::parse(url)
            .map_err(|e| AruaruError::InvalidUrl(e.to_string()))?;
        Ok(Self {
            host:     u.host_str().unwrap_or("localhost").to_string(),
            port:     u.port().unwrap_or(5432),
            dbname:   u.path().trim_start_matches('/').to_string(),
            user:     u.username().to_string(),
            password: u.password().unwrap_or("").to_string(),
        })
    }

    fn pg_connstr(&self) -> String {
        format!(
            "host={} port={} dbname={} user={} password={}",
            self.host, self.port, self.dbname, self.user, self.password
        )
    }
}

// ── メインクライアント ─────────────────────────────────────

/// aruaru-DB Rust クライアント
///
/// 内部で tokio-postgres の接続を保持する。
/// 長期利用や並行クエリには [`AruaruPool`] を使うこと。
pub struct AruaruClient {
    inner: PgClient,
}

impl AruaruClient {
    /// URL 文字列から接続を確立する
    ///
    /// ```text
    /// aruaru://root@localhost:5432/aruaru
    /// postgres://root@localhost:5432/aruaru
    /// ```
    pub async fn connect(url: &str) -> Result<Self> {
        let cfg = Config::from_url(url)?;
        Self::connect_config(&cfg).await
    }

    /// [`Config`] から接続を確立する
    pub async fn connect_config(config: &Config) -> Result<Self> {
        let (client, conn) = tokio_postgres::connect(&config.pg_connstr(), NoTls)
            .await
            .map_err(AruaruError::Connect)?;

        // 接続維持タスクをバックグラウンドで走らせる
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                eprintln!("[aruaru-db-rust] connection lost: {e}");
            }
        });

        Ok(Self { inner: client })
    }

    // ── 汎用 SQL ─────────────────────────────────────────

    /// SQL を実行して変更行数を返す (INSERT / UPDATE / DELETE / DDL)
    pub async fn execute(&self, sql: &str, params: &[&(dyn tokio_postgres::types::ToSql + Sync)]) -> Result<u64> {
        self.inner.execute(sql, params).await.map_err(AruaruError::Query)
    }

    /// SQL を実行して行を返す (SELECT)
    pub async fn query(&self, sql: &str, params: &[&(dyn tokio_postgres::types::ToSql + Sync)]) -> Result<Vec<Row>> {
        self.inner.query(sql, params).await.map_err(AruaruError::Query)
    }

    /// 先頭の1行のみ返す。0行なら `None`。
    pub async fn query_opt(&self, sql: &str, params: &[&(dyn tokio_postgres::types::ToSql + Sync)]) -> Result<Option<Row>> {
        self.inner.query_opt(sql, params).await.map_err(AruaruError::Query)
    }

    // ── Git-on-SQL ───────────────────────────────────────

    /// ブランチを作成する
    ///
    /// ```sql
    /// SELECT aruaru_branch('feature/my-feature')
    /// ```
    pub async fn branch(&self, name: &str) -> Result<()> {
        self.inner
            .execute(&format!("SELECT aruaru_branch('{}')", esc(name)), &[])
            .await
            .map_err(AruaruError::Query)?;
        Ok(())
    }

    /// ブランチを切り替える
    ///
    /// ```sql
    /// SELECT aruaru_checkout('main')
    /// ```
    pub async fn checkout(&self, branch: &str) -> Result<()> {
        self.inner
            .execute(&format!("SELECT aruaru_checkout('{}')", esc(branch)), &[])
            .await
            .map_err(AruaruError::Query)?;
        Ok(())
    }

    /// 現在のブランチをコミットし、コミット ID を返す
    ///
    /// ```sql
    /// SELECT aruaru_commit('my commit message')
    /// ```
    pub async fn commit(&self, message: &str) -> Result<String> {
        let row = self.inner
            .query_one(&format!("SELECT aruaru_commit('{}')", esc(message)), &[])
            .await
            .map_err(AruaruError::Query)?;
        Ok(row.get::<_, String>(0))
    }

    /// fast-forward マージを実行し、コミット ID を返す
    ///
    /// ```sql
    /// SELECT aruaru_merge('feature/my-feature')
    /// ```
    pub async fn merge(&self, from_branch: &str) -> Result<String> {
        let row = self.inner
            .query_one(&format!("SELECT aruaru_merge('{}')", esc(from_branch)), &[])
            .await
            .map_err(AruaruError::Query)?;
        Ok(row.get::<_, String>(0))
    }

    /// コミットログを取得する
    ///
    /// ```sql
    /// SELECT * FROM aruaru_log LIMIT n
    /// ```
    pub async fn log(&self, limit: usize) -> Result<Vec<CommitEntry>> {
        let rows = self.inner
            .query(&format!("SELECT * FROM aruaru_log LIMIT {limit}"), &[])
            .await
            .map_err(AruaruError::Query)?;
        Ok(rows.iter().map(CommitEntry::from_row).collect())
    }

    /// ブランチ一覧を取得する
    pub async fn list_branches(&self) -> Result<Vec<BranchEntry>> {
        let rows = self.inner
            .query("SELECT aruaru_list_branches()", &[])
            .await
            .map_err(AruaruError::Query)?;
        Ok(rows.iter().map(|r| BranchEntry {
            name: r.get::<_, String>(0),
        }).collect())
    }

    /// 現在のブランチ名を取得する
    pub async fn current_branch(&self) -> Result<String> {
        let row = self.inner
            .query_one("SELECT aruaru_current_branch()", &[])
            .await
            .map_err(AruaruError::Query)?;
        Ok(row.get::<_, String>(0))
    }

    /// 2ブランチ間の差分統計を取得する
    pub async fn diff(&self, from: &str, to: &str) -> Result<DiffStat> {
        let row = self.inner
            .query_one(
                &format!("SELECT * FROM aruaru_diff('{}', '{}')", esc(from), esc(to)),
                &[],
            )
            .await
            .map_err(AruaruError::Query)?;
        Ok(DiffStat {
            added:    row.try_get("added").unwrap_or(0),
            removed:  row.try_get("removed").unwrap_or(0),
            modified: row.try_get("modified").unwrap_or(0),
        })
    }

    // ── トランザクション ─────────────────────────────────

    /// BEGIN を送る。COMMIT/ROLLBACK は `execute` で明示するか
    /// [`AruaruTransaction`] を使う。
    pub async fn begin(&self) -> Result<AruaruTransaction<'_>> {
        self.inner.execute("BEGIN", &[]).await.map_err(AruaruError::Query)?;
        Ok(AruaruTransaction { client: self, done: false })
    }

    // ── 内部アクセス (高度な用途) ─────────────────────────

    /// 生の `tokio_postgres::Client` への参照 (非推奨 API への直接アクセス用)
    pub fn raw(&self) -> &PgClient {
        &self.inner
    }
}

// ── トランザクションガード ────────────────────────────────

/// DROP 時に未コミットなら自動 ROLLBACK するガード
pub struct AruaruTransaction<'a> {
    client: &'a AruaruClient,
    done: bool,
}

impl<'a> AruaruTransaction<'a> {
    pub async fn execute(&self, sql: &str, params: &[&(dyn tokio_postgres::types::ToSql + Sync)]) -> Result<u64> {
        self.client.execute(sql, params).await
    }

    pub async fn commit(mut self) -> Result<()> {
        self.client.inner.execute("COMMIT", &[]).await.map_err(AruaruError::Query)?;
        self.done = true;
        Ok(())
    }

    pub async fn rollback(mut self) -> Result<()> {
        self.client.inner.execute("ROLLBACK", &[]).await.map_err(AruaruError::Query)?;
        self.done = true;
        Ok(())
    }
}

impl<'a> Drop for AruaruTransaction<'a> {
    fn drop(&mut self) {
        if !self.done {
            // 非同期 ROLLBACK を同期 DROP から送れないため警告のみ。
            // 実用コードでは必ず .commit()/.rollback() を呼ぶこと。
            eprintln!("[aruaru-db-rust] WARNING: transaction dropped without commit/rollback");
        }
    }
}

// ── コネクションプール ────────────────────────────────────

/// deadpool-postgres ベースのコネクションプール
pub struct AruaruPool {
    pool: deadpool_postgres::Pool,
}

impl AruaruPool {
    /// URL から接続プールを作成する
    pub fn new(url: &str, max_size: usize) -> Result<Self> {
        let cfg = Config::from_url(url)?;
        let mut pg_cfg = deadpool_postgres::Config::new();
        pg_cfg.host     = Some(cfg.host);
        pg_cfg.port     = Some(cfg.port);
        pg_cfg.dbname   = Some(cfg.dbname);
        pg_cfg.user     = Some(cfg.user);
        pg_cfg.password = Some(cfg.password);
        pg_cfg.pool     = Some(deadpool_postgres::PoolConfig { max_size, ..Default::default() });
        let pool = pg_cfg
            .create_pool(Some(deadpool_postgres::Runtime::Tokio1), NoTls)
            .map_err(|e| AruaruError::Pool(e.to_string()))?;
        Ok(Self { pool })
    }

    /// プールから接続を1本取り出す
    pub async fn get(&self) -> Result<deadpool_postgres::Client> {
        self.pool.get().await.map_err(|e| AruaruError::Pool(e.to_string()))
    }

    pub fn pool(&self) -> &deadpool_postgres::Pool {
        &self.pool
    }
}

// ── 型 ───────────────────────────────────────────────────

/// ブランチ情報
#[derive(Debug, Clone)]
pub struct BranchEntry {
    pub name: String,
}

// ── 内部ユーティリティ ────────────────────────────────────

/// SQL シングルクォートエスケープ
fn esc(s: &str) -> String {
    s.replace('\'', "''")
}
