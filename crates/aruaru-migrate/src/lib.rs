//! aruaru-migrate: データベース移行ツール
//!
//! ## 対応ソース
//! - PostgreSQL (pg_dump COPY 形式)
//! - CockroachDB (PostgreSQL 互換エクスポート)
//! - Snowflake (Parquet export)
//! - MySQL / MariaDB (mysqldump)
//! - CSV / NDJSON

pub mod from_csv;
pub mod from_mysql;
pub mod from_parquet;
pub mod from_postgres;
pub mod schema_convert;
pub mod sql_build;
pub mod target;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// 移行元の種別
#[derive(Debug, Clone, Serialize, Deserialize, clap::ValueEnum)]
pub enum SourceKind {
    Postgres,
    Cockroach,
    Snowflake,
    Mysql,
    Csv,
    Ndjson,
    Parquet,
}

/// 移行設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrateConfig {
    pub source: SourceKind,
    /// 接続文字列 or ファイルパス
    pub source_uri: String,
    /// aruaru-DB 接続先
    pub target_uri: String,
    /// バッチサイズ (行数)
    pub batch_size: usize,
    /// コミットメッセージ
    pub commit_message: String,
    /// 並列度
    pub parallelism: usize,
}

impl Default for MigrateConfig {
    fn default() -> Self {
        Self {
            source: SourceKind::Postgres,
            source_uri: "postgres://user:pass@localhost/mydb".to_string(),
            target_uri: "postgres://root@localhost:5432/aruaru".to_string(),
            batch_size: 10_000,
            commit_message: "Migration import".to_string(),
            parallelism: 4,
        }
    }
}

/// 移行の進捗状態
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrateProgress {
    pub table: String,
    pub rows_total: Option<u64>,
    pub rows_done: u64,
    pub bytes_done: u64,
    pub elapsed_ms: u64,
    pub status: MigrateStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MigrateStatus {
    Pending,
    Running,
    Done,
    Failed(String),
}

/// 移行のエントリポイント
pub async fn run_migration(
    config: MigrateConfig,
    progress_cb: impl Fn(MigrateProgress) + Send + 'static,
) -> Result<()> {
    tracing::info!(source = ?config.source, "Starting migration");

    match config.source {
        SourceKind::Csv | SourceKind::Ndjson => {
            from_csv::migrate(&config, progress_cb).await?;
        }
        SourceKind::Postgres | SourceKind::Cockroach => {
            from_postgres::migrate(&config, progress_cb).await?;
        }
        // Snowflake は Parquet エクスポート経由での移行を前提とする
        // (`SourceKind::Parquet` と同じ読み込み経路を共有する)。
        SourceKind::Parquet | SourceKind::Snowflake => {
            from_parquet::migrate(&config, progress_cb).await?;
        }
        SourceKind::Mysql => {
            from_mysql::migrate(&config, progress_cb).await?;
        }
    }

    tracing::info!("Migration completed");
    Ok(())
}
