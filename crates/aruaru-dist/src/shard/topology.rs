//! クラスタトポロジ & シャードルーティング
//!
//! 複数の [`Range`] を束ね、キー → 担当 Range → Leader ノードへ解決する。
//! Range 分割、ノード配置、レプリカ不足の検出 (リバランス候補) を扱う。

use serde::{Deserialize, Serialize};

use super::Range;

/// クラスタ内のノード情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub node_id: u64,
    pub addr: String,
    /// 生存しているか (ハートビート由来)
    pub alive: bool,
}

/// ルーティング結果: あるキーを担当する Range と宛先
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteTarget {
    pub range_id: u64,
    pub leader: u64,
    pub replicas: Vec<u64>,
}

/// クラスタ全体のトポロジ
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClusterTopology {
    pub nodes: Vec<NodeInfo>,
    pub ranges: Vec<Range>,
    /// レプリケーション係数 (各 Range の目標レプリカ数)
    pub replication_factor: usize,
    next_range_id: u64,
}

impl ClusterTopology {
    /// 単一ノード・全域 1 Range で初期化
    pub fn single_node(node_id: u64, addr: impl Into<String>) -> Self {
        let mut t = Self {
            nodes: vec![NodeInfo { node_id, addr: addr.into(), alive: true }],
            ranges: Vec::new(),
            replication_factor: 1,
            next_range_id: 1,
        };
        t.ranges.push(Range {
            range_id: 1,
            start_key: None,
            end_key: None,
            replicas: vec![node_id],
            leader: node_id,
            size_bytes: 0,
        });
        t.next_range_id = 2;
        t
    }

    pub fn add_node(&mut self, node_id: u64, addr: impl Into<String>) {
        if !self.nodes.iter().any(|n| n.node_id == node_id) {
            self.nodes.push(NodeInfo { node_id, addr: addr.into(), alive: true });
        }
    }

    pub fn set_node_alive(&mut self, node_id: u64, alive: bool) {
        if let Some(n) = self.nodes.iter_mut().find(|n| n.node_id == node_id) {
            n.alive = alive;
        }
    }

    pub fn alive_nodes(&self) -> Vec<u64> {
        self.nodes.iter().filter(|n| n.alive).map(|n| n.node_id).collect()
    }

    /// キーを担当する Range を探す
    pub fn find_range(&self, key: &[u8]) -> Option<&Range> {
        self.ranges.iter().find(|r| r.contains(key))
    }

    /// キーのルーティング先 (Range + Leader + レプリカ)
    pub fn route(&self, key: &[u8]) -> Option<RouteTarget> {
        self.find_range(key).map(|r| RouteTarget {
            range_id: r.range_id,
            leader: r.leader,
            replicas: r.replicas.clone(),
        })
    }

    /// Range を指定キーで分割し、新 Range の id を返す
    pub fn split_range(&mut self, range_id: u64, split_key: Vec<u8>) -> Option<u64> {
        let pos = self.ranges.iter().position(|r| r.range_id == range_id)?;
        let new_id = self.next_range_id;
        let (left, right) = self.ranges[pos].split_at(split_key, new_id);
        self.ranges[pos] = left;
        self.ranges.insert(pos + 1, right);
        self.next_range_id += 1;
        Some(new_id)
    }

    /// 分割が必要な Range の一覧 (サイズ超過)
    pub fn ranges_needing_split(&self) -> Vec<u64> {
        self.ranges.iter().filter(|r| r.needs_split()).map(|r| r.range_id).collect()
    }

    /// レプリカ不足 (replication_factor 未満) の Range
    pub fn under_replicated(&self) -> Vec<u64> {
        self.ranges
            .iter()
            .filter(|r| r.replicas.len() < self.replication_factor)
            .map(|r| r.range_id)
            .collect()
    }

    /// リバランス候補: under-replicated な Range に、未保持の生存ノードを割り当てる提案。
    /// (range_id, 追加すべき node_id) のリストを返す (実際の追加は呼び出し側)。
    pub fn rebalance_plan(&self) -> Vec<(u64, u64)> {
        let alive = self.alive_nodes();
        let mut plan = Vec::new();
        for r in &self.ranges {
            if r.replicas.len() >= self.replication_factor {
                continue;
            }
            for &cand in &alive {
                if r.replicas.len() + plan.iter().filter(|(rid, _)| *rid == r.range_id).count()
                    >= self.replication_factor
                {
                    break;
                }
                if !r.replicas.contains(&cand) {
                    plan.push((r.range_id, cand));
                }
            }
        }
        plan
    }

    /// Range にレプリカノードを追加
    pub fn add_replica(&mut self, range_id: u64, node_id: u64) -> bool {
        if let Some(r) = self.ranges.iter_mut().find(|r| r.range_id == range_id) {
            if !r.replicas.contains(&node_id) {
                r.replicas.push(node_id);
                return true;
            }
        }
        false
    }

    pub fn range_count(&self) -> usize {
        self.ranges.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_node_route() {
        let t = ClusterTopology::single_node(1, "127.0.0.1:5432");
        let target = t.route(b"anykey").unwrap();
        assert_eq!(target.leader, 1);
        assert_eq!(target.range_id, 1);
    }

    #[test]
    fn test_split_and_route() {
        let mut t = ClusterTopology::single_node(1, "n1");
        let new_id = t.split_range(1, b"m".to_vec()).unwrap();
        assert_eq!(new_id, 2);
        assert_eq!(t.range_count(), 2);
        // "a" は左 Range(1), "z" は右 Range(2)
        assert_eq!(t.route(b"a").unwrap().range_id, 1);
        assert_eq!(t.route(b"z").unwrap().range_id, 2);
    }

    #[test]
    fn test_under_replication_and_rebalance() {
        let mut t = ClusterTopology::single_node(1, "n1");
        t.replication_factor = 3;
        t.add_node(2, "n2");
        t.add_node(3, "n3");
        // Range 1 は replica=[1] のみ → 不足
        assert_eq!(t.under_replicated(), vec![1]);
        let plan = t.rebalance_plan();
        // ノード 2,3 の追加が提案される
        assert_eq!(plan.len(), 2);
        for (rid, node) in plan {
            t.add_replica(rid, node);
        }
        assert!(t.under_replicated().is_empty());
    }

    #[test]
    fn test_node_liveness() {
        let mut t = ClusterTopology::single_node(1, "n1");
        t.add_node(2, "n2");
        t.set_node_alive(2, false);
        assert_eq!(t.alive_nodes(), vec![1]);
    }
}
