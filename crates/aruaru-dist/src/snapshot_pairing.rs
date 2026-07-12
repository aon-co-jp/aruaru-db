//! aruaru-db の Raft commit × open-raid-z (ZFS風) スナップショット連携。
//!
//! `open-web-server/CLAUDE.md` の「次回新規開発予定」節(2026-07-11、
//! ユーザー判断)で定義された機能の第一段実装。Raftのcommit(アプリケーション
//! 層のバージョニング = 実質Git-on-SQLコミット相当)に同期して、
//! ファイルシステム層(open-raid-z のスナップショット)のバージョニングを
//! 対応付けることで、両者にトランザクション単位の対応関係を持たせる。
//!
//! ## スコープ(この第一段実装で行うこと)
//! - Raftの `commit+適用完了` イベントごとに1回、スナップショット操作を
//!   トリガーする(`RaftNode::set_commit_hook` 経由)。
//! - `commit_index -> snapshot_id` の対応関係をメモリ上の
//!   `SnapshotPairingRegistry` に記録し、後から問い合わせ可能にする。
//!
//! ## スコープ外(将来の拡張)
//! - 双方向のリカバリ(スナップショットからのRaftログ巻き戻し等)は
//!   対象外。
//! - 対応関係の永続化(現状はプロセスメモリ上のみ。プロセス再起動で
//!   失われる)は対象外——将来は `aruaru-backup` の MANIFEST.json的な
//!   永続化と統合することが想定される。
//! - `open_raid_z_core` は個別の Cargo ワークスペース
//!   (`open-raid-z/open_runo_zfs_source/open_raid_z_core`)であり、本クレート
//!   からは `open_raid_z` feature 有効時のみ path 依存する
//!   (`--no-default-features` の `open_raid_z_core` を使うため WinFsp/dxc/
//!   Windows SDK 不要、CPUフォールバックのみで動作確認可能)。
//! - feature 無効時は `InMemorySnapshotBackend`(テスト・開発用のダミー実装)
//!   のみ利用可能。

use parking_lot::RwLock;
use std::sync::Arc;

use super::raft::{Applier, RaftNode};

/// スナップショット操作を抽象化するバックエンド。
/// 実体は `open-raid-z` の `Pool::create_snapshot` を呼ぶ実装
/// (`open_raid_z` feature)や、テスト用のインメモリ実装であり得る。
pub trait SnapshotBackend: Send + Sync {
    /// `label` という名前でスナップショットを作成し、成功したらその
    /// スナップショットID(名前)を返す。
    fn snapshot(&self, label: &str) -> Result<String, String>;
}

/// テスト・開発用のインメモリバックエンド。実際にはデータをコピーせず、
/// 呼ばれた回数とラベルだけを記録する(feature無効時のデフォルト検証経路)。
#[derive(Default)]
pub struct InMemorySnapshotBackend {
    created: RwLock<Vec<String>>,
}

impl InMemorySnapshotBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn created_snapshots(&self) -> Vec<String> {
        self.created.read().clone()
    }
}

impl SnapshotBackend for InMemorySnapshotBackend {
    fn snapshot(&self, label: &str) -> Result<String, String> {
        self.created.write().push(label.to_string());
        Ok(label.to_string())
    }
}

/// `commit_index -> snapshot_id` の対応関係を保持し、問い合わせ可能にする
/// レジストリ。
pub struct SnapshotPairingRegistry {
    backend: Arc<dyn SnapshotBackend>,
    mappings: RwLock<Vec<(u64, String)>>,
}

impl SnapshotPairingRegistry {
    pub fn new(backend: Arc<dyn SnapshotBackend>) -> Self {
        Self {
            backend,
            mappings: RwLock::new(Vec::new()),
        }
    }

    /// 指定された commit_index に対応するスナップショットを作成し、
    /// 対応関係を記録する。ラベルは `commit-<index>` とする。
    pub fn record_commit_snapshot(&self, commit_index: u64) -> Result<String, String> {
        let label = format!("commit-{commit_index}");
        let snapshot_id = self.backend.snapshot(&label)?;
        self.mappings
            .write()
            .push((commit_index, snapshot_id.clone()));
        Ok(snapshot_id)
    }

    /// 指定された commit_index に対応するスナップショットIDを問い合わせる
    /// (同じ commit_index に複数回スナップショットが記録された場合は
    /// 最後に記録されたものを返す)。
    pub fn snapshot_for_commit(&self, commit_index: u64) -> Option<String> {
        self.mappings
            .read()
            .iter()
            .rev()
            .find(|(idx, _)| *idx == commit_index)
            .map(|(_, id)| id.clone())
    }

    /// 記録済みの全対応関係(commit_index昇順ではなく記録順)。
    pub fn all_mappings(&self) -> Vec<(u64, String)> {
        self.mappings.read().clone()
    }
}

/// `RaftNode` の commit フックへ `SnapshotPairingRegistry` を配線する。
/// これ以降、Raftのcommit+適用が完了するたびに自動でスナップショットが
/// トリガーされ、`registry` に対応関係が記録される。
///
/// スナップショット作成の失敗はRaftの適用パイプライン自体を止めては
/// ならない(課金アイテム/金融データの書き込み成功をスナップショット
/// 失敗で巻き込まないため)ので、エラーは `tracing::warn!` に記録するのみ。
pub fn wire_to_node<A: Applier + 'static>(
    node: &Arc<RaftNode<A>>,
    registry: Arc<SnapshotPairingRegistry>,
) {
    node.set_commit_hook(move |commit_index| {
        if let Err(e) = registry.record_commit_snapshot(commit_index) {
            tracing::warn!(
                commit_index,
                error = %e,
                "aruaru-db commit x snapshot pairing: スナップショット作成に失敗(Raft適用自体は継続)"
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raft::{Command, CommandResponse};

    struct NoopApplier;
    impl Applier for NoopApplier {
        fn apply(&self, _command: &Command) -> CommandResponse {
            CommandResponse::ok()
        }
    }

    #[test]
    fn commit_triggers_snapshot_and_records_mapping() {
        let node = Arc::new(RaftNode::new(1, NoopApplier, vec![]));
        let backend = Arc::new(InMemorySnapshotBackend::new());
        let registry = Arc::new(SnapshotPairingRegistry::new(backend.clone()));
        wire_to_node(&node, registry.clone());

        node.become_leader();
        node.propose(&Command::Exec("INSERT 1".into())).unwrap();
        node.propose(&Command::Exec("INSERT 2".into())).unwrap();
        node.try_commit_to(2);
        assert_eq!(node.apply_committed(), 2);

        // commit_index=2 (適用済み最終インデックス) でスナップショットが
        // 1件作成され、対応関係が記録されている。
        assert_eq!(backend.created_snapshots(), vec!["commit-2".to_string()]);
        assert_eq!(
            registry.snapshot_for_commit(2),
            Some("commit-2".to_string())
        );
        assert_eq!(registry.all_mappings(), vec![(2, "commit-2".to_string())]);
    }

    #[test]
    fn snapshot_failure_does_not_break_raft_apply_pipeline() {
        struct FailingBackend;
        impl SnapshotBackend for FailingBackend {
            fn snapshot(&self, _label: &str) -> Result<String, String> {
                Err("simulated backend failure".to_string())
            }
        }

        let node = Arc::new(RaftNode::new(1, NoopApplier, vec![]));
        let registry = Arc::new(SnapshotPairingRegistry::new(Arc::new(FailingBackend)));
        wire_to_node(&node, registry.clone());

        node.become_leader();
        node.propose(&Command::Exec("INSERT 1".into())).unwrap();
        node.try_commit_to(1);
        // スナップショット作成が失敗しても、Raft側の適用件数は正しく報告される
        assert_eq!(node.apply_committed(), 1);
        assert!(registry.all_mappings().is_empty());
    }
}
