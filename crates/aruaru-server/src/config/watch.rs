//! `aruaru.yaml` のホットリロード。
//!
//! WunderGraph Cosmo の `watch_config`(ファイルの定期ポーリング)と
//! `SIGHUP` に倣う。新規依存を増やさないため、ファイル監視は mtime の
//! ポーリングで自前実装する(`notify` クレートの `PollWatcher` と実質同じ)。
//!
//! - `watch_config.enabled = false` なら監視タスクを起動しない。
//! - 変更検知 → [`AruaruConfig::load`] で再読込 → [`reconcile`] を適用。
//! - 解析エラーは error ログを出して**直前の設定を維持**(壊れた YAML を
//!   保存しても稼働中インスタンスは無事)。
//! - `cfg(unix)` では `SIGHUP` でも即時リロードする。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::admin::AdminState;

use super::{reconcile, AruaruConfig};

/// 設定監視タスクを起動する。`path` は監視対象の `aruaru.yaml`、
/// `initial` は起動時に読み込み済みの設定(差分検知の基準)。
pub fn spawn_config_watcher(path: PathBuf, initial: AruaruConfig, state: Arc<AdminState>) {
    if !initial.watch_config.enabled {
        tracing::info!("aruaru.yaml: watch_config.enabled = false のためホットリロードは無効");
        return;
    }
    let interval = Duration::from_millis(initial.watch_config.interval_ms.max(200));
    let startup_delay = Duration::from_millis(initial.watch_config.startup_delay_ms);

    tokio::spawn(async move {
        tokio::time::sleep(startup_delay).await;
        let mut last_mtime = mtime(&path);
        let mut current = initial;
        tracing::info!(
            path = %path.display(),
            interval_ms = interval.as_millis() as u64,
            "aruaru.yaml ホットリロード監視を開始"
        );

        #[cfg(unix)]
        let mut sighup = {
            use tokio::signal::unix::{signal, SignalKind};
            signal(SignalKind::hangup()).ok()
        };

        loop {
            let tick = tokio::time::sleep(interval);

            #[cfg(unix)]
            {
                let sighup_fut = async {
                    if let Some(s) = sighup.as_mut() {
                        s.recv().await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                };
                tokio::select! {
                    _ = tick => {}
                    _ = sighup_fut => {
                        tracing::info!("SIGHUP 受信: aruaru.yaml を再読込します");
                        reload(&path, &mut current, &state);
                        last_mtime = mtime(&path);
                        continue;
                    }
                }
            }
            #[cfg(not(unix))]
            {
                tick.await;
            }

            let now_mtime = mtime(&path);
            if now_mtime != last_mtime {
                last_mtime = now_mtime;
                reload(&path, &mut current, &state);
            }
        }
    });
}

fn mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn reload(path: &PathBuf, current: &mut AruaruConfig, state: &Arc<AdminState>) {
    match AruaruConfig::load(path) {
        Ok(new) => {
            let report = reconcile(&new, Some(current), state);
            if report.any_dynamic_change() {
                tracing::info!(?report, "aruaru.yaml のホットリロードを適用しました");
            } else if report.restart_required.is_empty() {
                tracing::debug!("aruaru.yaml に変更はありませんでした(内容は同一)");
            }
            *current = new;
        }
        Err(e) => {
            tracing::error!(error = %e, "aruaru.yaml の再読込に失敗。直前の設定を維持します");
        }
    }
}
