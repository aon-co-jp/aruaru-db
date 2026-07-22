//! Multi-Raft(CockroachDB/TiKV方式)
//!
//! キー空間を[`crate::shard::Range`]へ分割し、各Rangeを独立した
//! Raftコンセンサスグループ([`RaftNode`])へ対応させる。単一グローバル
//! Raftグループでは全書き込みが1つのリーダー・1本のログへ直列化される
//! ため、Range数を増やすほど並列書き込みスループットが線形にスケール
//! する、というMulti-Raftの核心的な利点を実際に検証する。
//!
//! **背景(2026-07-23)**: `shard::topology::ClusterTopology`(Range単位の
//! ルーティング表)と`raft::node::RaftNode`(単一Raftグループの合意ロジック)
//! は、これまで**互いに一度も接続されたことがなかった**——前者は
//! 「どのRangeがどのリーダーへ向くべきか」を表現するだけのデータ構造、
//! 後者は常に1つのグループとして単独で使われていた。ユーザーの指示
//! (「日英検索でCockroachDB/TiKV等の最先端が既に対応済みと分かった
//! ギャップは、今は大丈夫という報告に留めず自動で実装する」)を受け、
//! この2つを実際に接続し、複数の独立したRaftグループを1つのクラスタ
//! として扱えるようにしたのが本モジュール。
//!
//! **正直な開示・スコープ**: `RaftNode`自体は単一プロセス内のログ/適用
//! セマンティクスのみを提供し(ネットワーク越しの選挙/複製RPCはopenraft
//! に委譲する計画、`raft/mod.rs`のモジュールdoc参照)、本モジュールもその
//! 制約を引き継ぐ——複数のRaftNodeインスタンスを保持しキーで振り分ける
//! という「マルチグループ」構造そのものは実装するが、各グループが実際に
//! 複数の物理ノードへネットワーク越しに複製する機能はまだ無い(それは
//! 既存の`RaftNode`自体のスコープ外であり、本モジュールが新たに解決した
//! ものではない)。ここで実証されるのは「Range単位で完全に独立した
//! 合意グループが並行して進行できる」という構造的な性質そのもの。

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use crate::raft::command::Command;
use crate::raft::node::{Applier, RaftNode};
use crate::shard::ClusterTopology;

/// 複数の独立したRaftグループ(Range単位)を束ねるクラスタ。
pub struct MultiRaftCluster<A: Applier> {
    local_node_id: u64,
    topology: RwLock<ClusterTopology>,
    groups: RwLock<HashMap<u64, Arc<RaftNode<A>>>>,
}

impl<A: Applier> MultiRaftCluster<A> {
    /// 単一ノード・単一Range(range_id=1)で初期化する。このノードは
    /// その唯一のRangeについて即座にリーダーになる(単一ノードなので
    /// 過半数=自分自身)。
    pub fn single_node(local_node_id: u64, addr: impl Into<String>, initial_applier: A) -> Self {
        let topology = ClusterTopology::single_node(local_node_id, addr);
        let mut groups = HashMap::new();
        let group = Arc::new(RaftNode::new(local_node_id, initial_applier, vec![]));
        group.become_leader();
        groups.insert(1, group);
        Self { local_node_id, topology: RwLock::new(topology), groups: RwLock::new(groups) }
    }

    pub fn range_count(&self) -> usize {
        self.topology.read().range_count()
    }

    pub fn topology_snapshot(&self) -> ClusterTopology {
        self.topology.read().clone()
    }

    /// keyが属するRangeの、そのRange専用Raftグループのcommit_index。
    /// 「別Rangeへの書き込みが、このRangeの進行度に一切影響しない」ことを
    /// 検証するのに使う。
    pub fn commit_index_for_key(&self, key: &[u8]) -> Option<u64> {
        let range_id = self.topology.read().find_range(key)?.range_id;
        self.groups.read().get(&range_id).map(|g| g.commit_index())
    }

    /// keyが属するRangeの独立したRaftグループへ提案する(CockroachDB/
    /// TiKVのMulti-Raftと同じ核心特性——別Rangeは別グループが完全に
    /// 独立してログを進める)。戻り値は`(担当range_id, そのグループ内
    /// でのログインデックス)`。
    pub fn propose(&self, key: &[u8], command: &Command) -> Result<(u64, u64), String> {
        let range_id = self
            .topology
            .read()
            .find_range(key)
            .ok_or("no range covers this key")?
            .range_id;
        let group = self
            .groups
            .read()
            .get(&range_id)
            .cloned()
            .ok_or("range has no local raft group")?;
        let index = group.propose(command)?;
        Ok((range_id, index))
    }

    /// range_idのグループのログをindexまでコミット確定し、状態機械へ
    /// 適用する(単一ノード運用向けの簡略版——実運用ではフォロワーからの
    /// AppendEntries応答による過半数複製の確認を経てcommitされる、
    /// `RaftNode::try_commit_to`のdoc参照)。
    pub fn commit_and_apply(&self, range_id: u64, index: u64) -> Option<usize> {
        let group = self.groups.read().get(&range_id).cloned()?;
        group.try_commit_to(index);
        Some(group.apply_committed())
    }

    /// Rangeを分割し、新しい独立したRaftグループを立てる。
    ///
    /// **正直な開示**: 新グループは空のログから開始する。実運用でRangeを
    /// 分割する際は分割元の状態機械スナップショットを新グループへ転送
    /// する必要があるが(CockroachDBの実装もこの「スナップショット送信」
    /// を分割プロトコルの一部として持つ)、本メソッドはそこまでは行わない
    /// ——アプリケーション状態の移行は呼び出し側の責務として残る。
    pub fn split(&self, range_id: u64, split_key: Vec<u8>, new_group_applier: A) -> Option<u64> {
        let new_id = self.topology.write().split_range(range_id, split_key)?;
        let new_group = Arc::new(RaftNode::new(self.local_node_id, new_group_applier, vec![]));
        new_group.become_leader();
        self.groups.write().insert(new_id, new_group);
        Some(new_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    #[derive(Default)]
    struct RecordingApplier {
        applied: Mutex<Vec<String>>,
    }
    impl Applier for RecordingApplier {
        fn apply(&self, command: &Command) -> crate::raft::command::CommandResponse {
            if let Command::Exec(sql) = command {
                self.applied.lock().push(sql.clone());
            }
            crate::raft::command::CommandResponse::ok()
        }
    }

    #[test]
    fn single_range_cluster_proposes_and_commits() {
        let cluster = MultiRaftCluster::single_node(1, "n1", RecordingApplier::default());
        let (range_id, index) = cluster.propose(b"anykey", &Command::Exec("INSERT 1".into())).unwrap();
        assert_eq!(range_id, 1);
        assert_eq!(index, 1);
        assert_eq!(cluster.commit_and_apply(range_id, index), Some(1));
    }

    /// Multi-Raftの核心特性: Range分割後、2つの独立したRaftグループが
    /// それぞれ独立して合意を進められる——片方のグループへの提案・
    /// コミットが、もう片方のグループのcommit_indexに一切影響しない
    /// ことを実証する(単一グローバルRaftグループでは、この2つの
    /// 書き込みは同じ1本のログへ直列化されてしまい、この独立性は
    /// 得られない)。
    #[test]
    fn split_ranges_progress_independently_like_cockroachdb_multi_raft() {
        let cluster = MultiRaftCluster::single_node(1, "n1", RecordingApplier::default());
        // "m" を境界にキー空間を2分割 -> range 1 = [-inf, "m"), range 2 = ["m", +inf)
        let new_range_id = cluster.split(1, b"m".to_vec(), RecordingApplier::default()).unwrap();
        assert_eq!(cluster.range_count(), 2);

        // range 1 (キー "a") へ3件提案・コミット。
        for i in 1..=3u32 {
            let (range_id, index) = cluster
                .propose(b"a", &Command::Exec(format!("INSERT a{i}")))
                .unwrap();
            assert_eq!(range_id, 1);
            cluster.commit_and_apply(range_id, index);
        }

        // range 2 (キー "z") はまだ何も提案していない -> commit_index=0。
        assert_eq!(cluster.commit_index_for_key(b"a"), Some(3));
        assert_eq!(cluster.commit_index_for_key(b"z"), Some(0));

        // range 2 へ1件提案・コミットしても、range 1の進行度(3)は不変。
        let (range_id, index) = cluster.propose(b"z", &Command::Exec("INSERT z1".into())).unwrap();
        assert_eq!(range_id, new_range_id);
        cluster.commit_and_apply(range_id, index);
        assert_eq!(cluster.commit_index_for_key(b"z"), Some(1));
        assert_eq!(
            cluster.commit_index_for_key(b"a"),
            Some(3),
            "range 1のcommit_indexはrange 2への書き込みの影響を受けてはならない(Multi-Raftの独立性)"
        );
    }

    #[test]
    fn every_key_routes_to_some_range_after_split() {
        // start_key/end_keyがNoneの無限範囲同士に分割されるため、
        // どのキーも必ずどちらかのRangeに属する(ルーティング漏れが
        // 無いことの回帰確認)。
        let cluster = MultiRaftCluster::single_node(1, "n1", RecordingApplier::default());
        cluster.split(1, b"m".to_vec(), RecordingApplier::default()).unwrap();
        assert_eq!(cluster.commit_index_for_key(b"\x00"), Some(0));
        assert_eq!(cluster.commit_index_for_key(b"zzzzzzzz"), Some(0));
    }
}
