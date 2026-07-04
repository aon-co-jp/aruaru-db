//! PostgreSQL / CockroachDB 移行 (COPY 形式)
use crate::{MigrateConfig, MigrateProgress};
pub async fn migrate(
    config: &MigrateConfig,
    progress_cb: impl Fn(MigrateProgress) + Send + 'static,
) -> anyhow::Result<()> {
    // TODO: tokio-postgres で接続 → COPY TO stdout → aruaru-wire で ingest
    tracing::info!(uri = %config.source_uri, "Postgres migration: TODO");
    Ok(())
}
