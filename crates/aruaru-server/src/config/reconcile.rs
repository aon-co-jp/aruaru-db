//! `AruaruConfig` の**動的セクション**を稼働中の [`AdminState`] へ適用する。
//!
//! 設計の正本は [`docs/CONTROL_PLANE_REDESIGN.md`] §6。
//!
//! - 「望ましい状態」を表す宣言的設定なので、reconcile は**冪等**
//!   (同じ config を何度適用しても同じ結果)。
//! - **静的セクション**(`server` / `raft`)は変更を検知したら warn ログを
//!   出すだけ(プロセス再起動が必要。Cosmo と同じ制約)。
//! - P1 の適用対象は `backup.schedule` と `federation.sources`。
//!   `query.parallel` / `follower_read` / `wal` / `sharded_store` は
//!   P2 以降で `AdminState` 側の受け皿を整えてから接続する。

use std::sync::Arc;

use aruaru_dist::admin_shared::{BackupScheduleState, FederatedSourceEntry, ParallelConfigState};

use crate::admin::AdminState;

use super::AruaruConfig;

/// reconcile 1 回で実際に変わった項目の要約(ログ/テスト用)。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    pub schedule_changed: bool,
    pub federation_changed: bool,
    pub parallel_changed: bool,
    pub follower_read_lag_changed: bool,
    /// 静的セクション(server / raft / wal / sharded_store)に差分があり
    /// 「要再起動」を警告した項目名。`wal` と `sharded_store` は safekeeper
    /// 台数・シャード数が構築時固定で、稼働中の再構成は進行中状態を
    /// 失うため**意図的に静的扱い**とする(Cosmo の静的セクションと同じ)。
    pub restart_required: Vec<String>,
}

impl ReconcileReport {
    pub fn any_dynamic_change(&self) -> bool {
        self.schedule_changed
            || self.federation_changed
            || self.parallel_changed
            || self.follower_read_lag_changed
    }
}

/// 新しい config を `AdminState` へ適用する。`previous` は直前に適用した
/// config(静的セクションの差分検知に使う。初回は `None`)。
pub fn reconcile(
    new: &AruaruConfig,
    previous: Option<&AruaruConfig>,
    state: &Arc<AdminState>,
) -> ReconcileReport {
    let mut report = ReconcileReport::default();

    // ── 静的セクション: 差分があれば warn のみ ───────────────
    if let Some(prev) = previous {
        for (name, differs) in [
            ("server.pg_port", prev.server.pg_port != new.server.pg_port),
            ("server.graphql_port", prev.server.graphql_port != new.server.graphql_port),
            ("server.data_dir", prev.server.data_dir != new.server.data_dir),
            ("server.tls", !tls_eq(&prev.server.tls, &new.server.tls)),
            ("raft.node_id", prev.raft.node_id != new.raft.node_id),
            ("raft.role", prev.raft.role != new.raft.role),
            ("raft.peers", prev.raft.peers != new.raft.peers),
            // wal / sharded_store は構築時固定(理由は ReconcileReport の doc)。
            ("wal.safekeepers", prev.wal.safekeepers != new.wal.safekeepers),
            ("wal.quorum", prev.wal.quorum != new.wal.quorum),
            ("sharded_store.shards", prev.sharded_store.shards != new.sharded_store.shards),
            // htap は --columnar-learner の起動可否・読み取り検証方式・
            // ColumnarApplier の compaction 閾値を決めるため構築時固定。
            ("htap", prev.htap != new.htap),
        ] {
            if differs {
                tracing::warn!(
                    section = name,
                    "aruaru.yaml の静的セクションが変更されました。反映にはプロセス再起動が必要です(ホットリロード対象外)。"
                );
                report.restart_required.push(name.to_string());
            }
        }
    }

    // ── query.parallel(4フィールド、完全ホットリロード) ────
    {
        let desired = ParallelConfigState {
            enabled: new.query.parallel.enabled,
            max_workers: new.query.parallel.max_workers,
            chunk_size: new.query.parallel.chunk_size,
            strategy: new.query.parallel.strategy.clone(),
        };
        let handle = state.parallel_handle();
        let mut cur = handle.lock();
        if *cur != desired {
            *cur = desired;
            report.parallel_changed = true;
            tracing::info!(?cur, "aruaru.yaml: query.parallel を反映しました");
        }
    }

    // ── follower_read.target_lag_ms(closed timestamp、完全ホットリロード) ──
    {
        let desired_nanos = new.follower_read.target_lag_ms.saturating_mul(1_000_000);
        let coord = state.closed_ts_coordinator();
        if coord.target_lag_nanos() != desired_nanos {
            coord.set_target_lag_nanos(desired_nanos);
            report.follower_read_lag_changed = true;
            tracing::info!(
                target_lag_ms = new.follower_read.target_lag_ms,
                "aruaru.yaml: follower_read.target_lag_ms を全 tracker へ反映しました"
            );
        }
        // HLC クロックスキュー上限(max_offset)。
        let desired_offset_nanos = new.follower_read.max_offset_ms.saturating_mul(1_000_000);
        let hlc = state.hlc_handle();
        if hlc.max_offset_nanos() != desired_offset_nanos {
            hlc.set_max_offset_nanos(desired_offset_nanos);
            report.follower_read_lag_changed = true;
            tracing::info!(
                max_offset_ms = new.follower_read.max_offset_ms,
                "aruaru.yaml: follower_read.max_offset_ms を HLC へ反映しました"
            );
        }
    }

    // ── backup.schedule ─────────────────────────────────────
    {
        let desired = if new.backup.schedule.enabled
            || previous.map(|p| p.backup.schedule.enabled).unwrap_or(false)
        {
            Some(BackupScheduleState {
                enabled: new.backup.schedule.enabled,
                cron: new.backup.schedule.cron.clone(),
                kind: new.backup.schedule.kind.clone(),
                updated_at: chrono::Utc::now().to_rfc3339(),
            })
        } else {
            None
        };
        let handle = state.schedule_handle();
        let mut cur = handle.lock();
        if !schedule_eq(cur.as_ref(), desired.as_ref()) {
            *cur = desired;
            report.schedule_changed = true;
            tracing::info!("aruaru.yaml: backup.schedule を反映しました");
        }
    }

    // ── federation.sources ──────────────────────────────────
    {
        let desired: Vec<FederatedSourceEntry> = new
            .federation
            .sources
            .iter()
            .map(|s| FederatedSourceEntry {
                name: s.name.clone(),
                kind: s.kind.clone(),
                uri: s.uri.clone(),
                read_only: s.read_only,
                pushdown: s.pushdown,
                status: Some("unknown".into()),
                table_count: None,
            })
            .collect();
        let handle = state.federation_handle();
        let mut cur = handle.lock();
        if !federation_eq(&cur, &desired) {
            *cur = desired;
            report.federation_changed = true;
            tracing::info!(
                count = cur.len(),
                "aruaru.yaml: federation.sources を反映しました"
            );
        }
    }

    report
}

fn tls_eq(a: &super::TlsConfig, b: &super::TlsConfig) -> bool {
    a.cert == b.cert && a.key == b.key && a.client_ca == b.client_ca
}

fn schedule_eq(a: Option<&BackupScheduleState>, b: Option<&BackupScheduleState>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x.enabled == y.enabled && x.cron == y.cron && x.kind == y.kind,
        _ => false,
    }
}

fn federation_eq(a: &[FederatedSourceEntry], b: &[FederatedSourceEntry]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.name == y.name
                && x.kind == y.kind
                && x.uri == y.uri
                && x.read_only == y.read_only
                && x.pushdown == y.pushdown
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aruaru_query::QueryEngine;

    fn state() -> Arc<AdminState> {
        AdminState::new(Arc::new(QueryEngine::new()), aruaru_registry::Registry::new())
    }

    fn cfg_from(y: &str) -> AruaruConfig {
        serde_norway::from_str(y).unwrap()
    }

    #[test]
    fn applies_schedule_and_federation_then_is_idempotent() {
        let st = state();
        let cfg = cfg_from(
            r#"
backup:
  schedule:
    enabled: true
    cron: "0 2 * * *"
    kind: "full"
federation:
  sources:
    - name: "wh"
      kind: "postgres"
      uri: "postgres://x/wh"
"#,
        );

        let r1 = reconcile(&cfg, None, &st);
        assert!(r1.schedule_changed && r1.federation_changed);
        assert_eq!(st.schedule_handle().lock().as_ref().unwrap().cron, "0 2 * * *");
        assert_eq!(st.federation_handle().lock().len(), 1);

        // 同じ config をもう一度 → 何も変わらない(冪等)。
        let r2 = reconcile(&cfg, Some(&cfg), &st);
        assert!(!r2.any_dynamic_change(), "reconcile は冪等であるべき: {r2:?}");
    }

    #[test]
    fn removing_a_federation_source_from_yaml_removes_it_from_state() {
        let st = state();
        let with = cfg_from(
            r#"
federation:
  sources:
    - { name: "a", kind: "postgres", uri: "postgres://x/a" }
    - { name: "b", kind: "mysql", uri: "mysql://x/b" }
"#,
        );
        reconcile(&with, None, &st);
        assert_eq!(st.federation_handle().lock().len(), 2);

        let without = cfg_from("{}");
        let r = reconcile(&without, Some(&with), &st);
        assert!(r.federation_changed);
        assert!(st.federation_handle().lock().is_empty());
    }

    #[test]
    fn static_section_change_reports_restart_required() {
        let st = state();
        let a = cfg_from("server:\n  pg_port: 5432\n");
        let b = cfg_from("server:\n  pg_port: 5999\n");
        let r = reconcile(&b, Some(&a), &st);
        assert!(r.restart_required.iter().any(|s| s == "server.pg_port"));
    }

    #[test]
    fn wal_and_sharded_store_changes_are_restart_required_not_hot() {
        let st = state();
        let a = cfg_from("wal:\n  quorum: 2\nsharded_store:\n  shards: 0\n");
        let b = cfg_from("wal:\n  quorum: 3\nsharded_store:\n  shards: 8\n");
        let r = reconcile(&b, Some(&a), &st);
        assert!(r.restart_required.iter().any(|s| s == "wal.quorum"));
        assert!(r.restart_required.iter().any(|s| s == "sharded_store.shards"));
        assert!(!r.any_dynamic_change());
    }

    #[test]
    fn parallel_config_hot_reloads_into_admin_state() {
        let st = state();
        let cfg = cfg_from(
            r#"
query:
  parallel:
    enabled: true
    max_workers: 12
    chunk_size: 5000
    strategy: "range"
"#,
        );
        let r = reconcile(&cfg, None, &st);
        assert!(r.parallel_changed);
        let cur = st.parallel_handle().lock().clone();
        assert!(cur.enabled);
        assert_eq!(cur.max_workers, 12);
        assert_eq!(cur.chunk_size, 5000);
        assert_eq!(cur.strategy, "range");

        // 冪等
        let r2 = reconcile(&cfg, Some(&cfg), &st);
        assert!(!r2.parallel_changed);
    }

    #[test]
    fn follower_read_target_lag_hot_reloads_into_coordinator() {
        let st = state();
        let coord = st.closed_ts_coordinator();
        // 既存の tracker を1つ作っておき、変更が既存 tracker にも効くことを確認。
        let tracker = coord.register_range(1);
        assert_eq!(coord.target_lag_nanos(), 3_000_000_000);
        assert_eq!(tracker.target_lag_nanos(), 3_000_000_000);

        let cfg = cfg_from("follower_read:\n  target_lag_ms: 500\n");
        let r = reconcile(&cfg, None, &st);
        assert!(r.follower_read_lag_changed);
        assert_eq!(coord.target_lag_nanos(), 500_000_000);
        // 同一の Arc<AtomicU64> を共有しているので既存 tracker にも即反映。
        assert_eq!(tracker.target_lag_nanos(), 500_000_000);

        let r2 = reconcile(&cfg, Some(&cfg), &st);
        assert!(!r2.follower_read_lag_changed);
    }
}
