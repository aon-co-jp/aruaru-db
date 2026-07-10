//! 毎日の自動クロールスケジューラ
//!
//! 起動直後に 1 回クロールし、以後 24 時間ごとに繰り返す。
//! aruaru-server から spawn して常駐させる。

use std::sync::Arc;
use std::time::Duration;

use crate::registry::Registry;

/// 24 時間周期
const DAILY: Duration = Duration::from_secs(24 * 60 * 60);

/// 毎日クロールを回し続ける (無限ループ)。tokio タスクとして spawn する。
pub async fn run_daily(registry: Arc<Registry>) {
    // 起動直後に初回クロール (失敗しても継続)
    crawl_once(&registry).await;

    let mut ticker = tokio::time::interval(DAILY);
    ticker.tick().await; // 1 回目は即時消化済み扱い
    loop {
        ticker.tick().await;
        crawl_once(&registry).await;
    }
}

async fn crawl_once(registry: &Arc<Registry>) {
    match registry.crawl_now().await {
        Ok(report) => tracing::info!(
            crawled = report.crawled,
            matched = report.matched,
            at = %report.crawled_at,
            "daily registry crawl done"
        ),
        Err(e) => tracing::warn!(error = %e, "daily registry crawl failed"),
    }
}
