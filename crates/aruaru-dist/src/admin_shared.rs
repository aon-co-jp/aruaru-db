//! REST(`aruaru-server::admin::AdminState`)とGraphQL
//! (`aruaru-graphql::admin_resolvers::AdminCtx`)が**同一インスタンス**を
//! 共有するための管理状態の型。
//!
//! # 背景(2026-08-29のREST→GraphQL段階移行方針)
//!
//! `cluster_status`(`ClusterTopology`、既に実装済み)と同じパターンを
//! 他の運用系データへも適用する。従来、GraphQL側の`backup_schedule`・
//! `federated_sources`はREST側の実状態(`AdminState.schedule`/
//! `.federation`)を一切参照せず、常に固定値(`None`/空配列)を返す
//! スタブだった——「GraphQL対応済み」に見えて実際は別の宇宙のデータ、
//! という不整合を解消するため、両者が読み書きする構造体をこの
//! (両クレートが依存する)`aruaru-dist`クレートへ切り出した。
//!
//! `aruaru-server`は`Arc<Mutex<..>>`をフィールドに持ち、
//! `aruaru-graphql::AdminCtx`へその同じ`Arc`を渡すことで、REST経由の
//! 書き込みがGraphQL読み取りへ即座に反映され、その逆も成り立つ。

use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// バックアップの定期実行スケジュール設定。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackupScheduleState {
    pub enabled: bool,
    pub cron: String,
    pub kind: String,
    pub updated_at: String,
}

/// `Option<BackupScheduleState>`を共有するハンドル(未設定時は`None`)。
pub type SharedBackupSchedule = Arc<Mutex<Option<BackupScheduleState>>>;

/// フェデレーション(外部DBソース)登録エントリ。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FederatedSourceEntry {
    pub name: String,
    pub kind: String,
    pub uri: String,
    pub read_only: bool,
    pub pushdown: bool,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub table_count: Option<u32>,
}

/// 登録済みフェデレーションソース一覧を共有するハンドル。
pub type SharedFederatedSources = Arc<Mutex<Vec<FederatedSourceEntry>>>;

/// 並列/分散クエリ実行の設定(**GraphQL `ParallelConfigGql` と同じ4
/// フィールド**)。
///
/// # 背景(2026-08-29 再設計 P2)
///
/// 従来 REST 側(`aruaru-server::admin` の独自 `ParallelConfig`、7
/// フィールド)と GraphQL 側(4フィールド・スタブ)でスキーマが非互換
/// だった。ユーザー判断により「GraphQL の4フィールド
/// (`enabled`/`max_workers`/`chunk_size`/`strategy`)を正とし REST を
/// そこへ寄せる」方針が確定。再設計ではこの設定は宣言的
/// `aruaru.yaml: query.parallel` が正本で、`reconcile` がここへ書き込み、
/// GraphQL `parallelConfig` query と REST `/admin/parallel/explain` が
/// ここを読む(`setParallelConfig` mutation・`GET/POST /admin/parallel`
/// は撤廃)。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParallelConfigState {
    pub enabled: bool,
    pub max_workers: u32,
    pub chunk_size: u32,
    pub strategy: String,
}

impl Default for ParallelConfigState {
    fn default() -> Self {
        Self { enabled: false, max_workers: 4, chunk_size: 10_000, strategy: "hash".into() }
    }
}

/// `ParallelConfigState` を共有するハンドル。
pub type SharedParallelConfig = Arc<Mutex<ParallelConfigState>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_handle_shares_mutations_across_clones() {
        let handle: SharedBackupSchedule = Arc::new(Mutex::new(None));
        let cloned = handle.clone();
        *handle.lock() = Some(BackupScheduleState {
            enabled: true,
            cron: "0 3 * * *".into(),
            kind: "full".into(),
            updated_at: "2026-08-29T00:00:00Z".into(),
        });
        assert_eq!(cloned.lock().as_ref().unwrap().cron, "0 3 * * *");
    }

    #[test]
    fn federation_handle_shares_mutations_across_clones() {
        let handle: SharedFederatedSources = Arc::new(Mutex::new(Vec::new()));
        let cloned = handle.clone();
        handle.lock().push(FederatedSourceEntry {
            name: "warehouse".into(),
            kind: "postgres".into(),
            uri: "postgres://example/db".into(),
            read_only: true,
            pushdown: false,
            status: Some("unknown".into()),
            table_count: None,
        });
        assert_eq!(cloned.lock().len(), 1);
        assert_eq!(cloned.lock()[0].name, "warehouse");
    }
}
