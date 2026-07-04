//! CSV / NDJSON 移行
use crate::{MigrateConfig, MigrateProgress};
pub async fn migrate(
    config: &MigrateConfig,
    progress_cb: impl Fn(MigrateProgress) + Send + 'static,
) -> anyhow::Result<()> {
    // TODO: Arrow CSV reader でストリーミング読み込み → aruaru-wire 経由で ingest
    tracing::info!(uri = %config.source_uri, "CSV migration: TODO");
    Ok(())
}
