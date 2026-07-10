//! MySQL / MariaDB 移行 (ワイヤ互換読み出し → aruaru-DB へ INSERT)
//!
//! 読み出しは aruaru-registry の `MySqlAdapter` (mysql_async 実接続) を
//! 再利用する。DDL の型差異 (`schema_convert::mysql_to_aruaru`) は、この
//! エンジンが全列 TEXT を前提とする v0.5 時点では列名のみを使うため
//! 直接は適用しないが、将来型付きスキーマへ拡張する際の変換先として残す。
use crate::target::TargetClient;
use crate::{MigrateConfig, MigrateProgress, MigrateStatus};
use aruaru_registry::adapter::{MySqlAdapter, SourceAdapter};

pub async fn migrate(
    config: &MigrateConfig,
    progress_cb: impl Fn(MigrateProgress) + Send + 'static,
) -> anyhow::Result<()> {
    tracing::info!(uri = %config.source_uri, "MySQL migration: starting");

    let adapter = MySqlAdapter;
    let tables = adapter.list_tables(&config.source_uri).await?;
    let target = TargetClient::connect(&config.target_uri).await?;

    for t in &tables {
        progress_cb(MigrateProgress {
            table: t.name.clone(),
            rows_total: Some(t.estimated_rows.max(0) as u64),
            rows_done: 0,
            bytes_done: 0,
            elapsed_ms: 0,
            status: MigrateStatus::Running,
        });

        let (columns, rows) = adapter
            .read_table(&config.source_uri, &t.schema, &t.name, config.batch_size)
            .await?;
        if columns.is_empty() {
            tracing::warn!(table = %t.name, "no columns returned, skipping table");
            continue;
        }

        target.ensure_table(&t.name, &columns).await?;
        let done = target.insert_rows(&t.name, &rows).await;

        progress_cb(MigrateProgress {
            table: t.name.clone(),
            rows_total: Some(rows.len() as u64),
            rows_done: done as u64,
            bytes_done: 0,
            elapsed_ms: 0,
            status: MigrateStatus::Done,
        });
    }

    tracing::info!(tables = tables.len(), "MySQL migration: done");
    Ok(())
}
