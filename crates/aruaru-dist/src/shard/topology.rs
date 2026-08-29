//! クラスタトポロジ & シャードルーティング
//!
//! 複数の [`Range`] を束ね、キー → 担当 Range → Leader ノードへ解決する。
//! Range 分割、ノード配置、レプリカ不足の検出 (リバランス候補) を扱う。

use serde::{Deserialize, Serialize};

use super::Range;

/// 【2026-08-29新設】クラスタ状態の要約(REST `/admin/cluster`とGraphQL
/// `clusterStatus`の**両方**がこの1つの関数を呼ぶことで、見せかけの
/// GraphQL側実装(固定値`total_nodes: 1`等を返すだけのスタブ)を
/// 解消し、実際のトポロジを単一の実装から正しく反映するようにした
/// (ユーザー指示「/admin/*の運用系REST操作のうちデータ寄りのものを
/// GraphQLへ移し、RESTの必要性を実際に減らす」への対応の第一歩)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStatusSnapshot {
    pub total_nodes: usize,
    pub healthy_nodes: usize,
    pub total_ranges: usize,
    pub total_rows: u64,
    pub table_count: usize,
    pub replication_factor: usize,
    pub under_replicated: Vec<u64>,
    pub nodes: Vec<NodeStatusSnapshot>,
    pub ranges: Vec<RangeStatusSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatusSnapshot {
    pub node_id: u64,
    pub addr: String,
    pub role: String,
    pub alive: bool,
    pub commit_index: u64,
    pub applied_index: u64,
    pub ranges: usize,
    pub disk_used_gb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeStatusSnapshot {
    pub range_id: u64,
    pub start_key: String,
    pub end_key: String,
    pub leader_node: u64,
    pub replicas: Vec<u64>,
    pub size_mb: f64,
}

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

    /// クラスタ状態の要約を組み立てる(REST `/admin/cluster`・GraphQL
    /// `clusterStatus`共通)。`commit_count`/`total_rows`/`table_count`
    /// (`QueryEngine`から取得、このクレートは`aruaru-query`に依存しない
    /// ため呼び出し側が渡す)以外は、すべてこのトポロジ自身が持つ情報
    /// から導出する。
    pub fn status_snapshot(
        &self,
        commit_count: u64,
        total_rows: u64,
        table_count: usize,
    ) -> ClusterStatusSnapshot {
        let alive = self.alive_nodes();
        let nodes = self
            .nodes
            .iter()
            .map(|n| {
                let is_leader = self.ranges.iter().any(|r| r.leader == n.node_id);
                let range_cnt = self.ranges.iter().filter(|r| r.replicas.contains(&n.node_id)).count();
                NodeStatusSnapshot {
                    node_id: n.node_id,
                    addr: n.addr.clone(),
                    role: if is_leader { "Leader".to_string() } else { "Follower".to_string() },
                    alive: n.alive,
                    commit_index: commit_count,
                    applied_index: commit_count,
                    ranges: range_cnt,
                    disk_used_gb: (total_rows as f64 * 64.0) / 1e9,
                }
            })
            .collect();
        let ranges = self
            .ranges
            .iter()
            .map(|r| RangeStatusSnapshot {
                range_id: r.range_id,
                start_key: r
                    .start_key
                    .as_ref()
                    .map(|k| String::from_utf8_lossy(k).to_string())
                    .unwrap_or_else(|| "(min)".into()),
                end_key: r
                    .end_key
                    .as_ref()
                    .map(|k| String::from_utf8_lossy(k).to_string())
                    .unwrap_or_else(|| "(max)".into()),
                leader_node: r.leader,
                replicas: r.replicas.clone(),
                size_mb: (r.size_bytes as f64) / 1e6,
            })
            .collect();
        ClusterStatusSnapshot {
            total_nodes: self.nodes.len(),
            healthy_nodes: alive.len(),
            total_ranges: self.ranges.len(),
            total_rows,
            table_count,
            replication_factor: self.replication_factor,
            under_replicated: self.under_replicated(),
            nodes,
            ranges,
        }
    }

    /// 【2026-08-21新設・Vitess再検証で発見した実欠落への対応】隣接する2つの
    /// Rangeを1つに統合する(Vitessの"Reshard"が持つシャード併合〈複数シャードを
    /// 1つへ戻す〉、CockroachDBのRange Mergeにも相当する操作)。
    ///
    /// 【見送り再検証(2026-08-21)の経緯】前回HANDOFFはVitessを「VTGate相当は
    /// 既にキー空間分離で代替済み」として見送ったが、実際に`ClusterTopology`の
    /// コードを読み直した結果、**分割(`split_range`)は実装済みだが併合は
    /// 一件も無かった**——Vitessのドキュメント
    /// (https://vitess.io/docs/reference/vreplication/reshard/)が明記する
    /// 「シャード数を増やすことも減らすこともできる双方向の操作」のうち
    /// 半分しか実装されていなかった、という具体的な欠落だったため、
    /// 「既に代替済み」という前回の見送り理由は不正確だったと判明し、今回
    /// 実装した。
    ///
    /// **前提**: 2つのRangeが隣接している(`a.end_key == b.start_key`、
    /// またはその逆)こと。隣接していない場合は`None`を返す(キー空間の
    /// 連続性が崩れるため)。統合後のRangeは`range_id`が小さい方を引き継ぎ、
    /// レプリカ集合は両者の和集合(順序を保ちつつ重複除去)、Leaderは
    /// 引き継いだ側のLeaderをそのまま使う(実運用では統合直後に再選挙が
    /// 走る想定、ここでは構造的な統合のみを扱う——`split`が新グループを
    /// 空ログから始める簡略化と対になる簡略化)。
    pub fn merge_ranges(&mut self, range_a: u64, range_b: u64) -> Option<u64> {
        if range_a == range_b {
            return None;
        }
        let pos_a = self.ranges.iter().position(|r| r.range_id == range_a)?;
        let pos_b = self.ranges.iter().position(|r| r.range_id == range_b)?;

        let (left_pos, right_pos) = if pos_a < pos_b { (pos_a, pos_b) } else { (pos_b, pos_a) };
        // 隣接性チェック: 左側のend_keyと右側のstart_keyが一致することを要求
        // (配列上も隣り合っている必要がある——飛び地の統合は許さない)。
        if right_pos != left_pos + 1 {
            return None;
        }
        if self.ranges[left_pos].end_key != self.ranges[right_pos].start_key {
            return None;
        }

        let left = self.ranges[left_pos].clone();
        let right = self.ranges.remove(right_pos);

        let merged_id = left.range_id.min(right.range_id);
        let keep_left_meta = merged_id == left.range_id;

        let mut replicas = left.replicas.clone();
        for r in &right.replicas {
            if !replicas.contains(r) {
                replicas.push(*r);
            }
        }

        let merged = super::Range {
            range_id: merged_id,
            start_key: left.start_key.clone(),
            end_key: right.end_key.clone(),
            replicas,
            leader: if keep_left_meta { left.leader } else { right.leader },
            size_bytes: left.size_bytes + right.size_bytes,
        };
        self.ranges[left_pos] = merged;
        Some(merged_id)
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

    /// 【Vitess Reshard(併合方向)の検証】分割した2つのRangeを再び1つへ
    /// 統合すると、Rangeが1件に戻り、統合後のRangeが分割前と同じキー空間
    /// (start_key/end_key)全体をカバーする。
    #[test]
    fn test_merge_reverses_split_like_vitess_reshard() {
        let mut t = ClusterTopology::single_node(1, "n1");
        t.add_node(2, "n2");
        let new_id = t.split_range(1, b"m".to_vec()).unwrap();
        assert_eq!(t.range_count(), 2);
        // 右側(new_id)にだけレプリカを追加(統合後にレプリカ集合が和集合になることの検証用)
        t.add_replica(new_id, 2);

        let merged_id = t.merge_ranges(1, new_id).unwrap();
        assert_eq!(t.range_count(), 1);
        assert_eq!(merged_id, 1); // range_idが小さい方を引き継ぐ

        // 統合後のRangeが分割前と同じ全域をカバーする(どのキーも同じRangeへ解決)
        assert_eq!(t.route(b"a").unwrap().range_id, merged_id);
        assert_eq!(t.route(b"z").unwrap().range_id, merged_id);

        // レプリカは両者の和集合
        let merged_range = t.ranges.iter().find(|r| r.range_id == merged_id).unwrap();
        assert!(merged_range.replicas.contains(&1));
        assert!(merged_range.replicas.contains(&2));
    }

    /// 隣接していないRange同士の統合は拒否される(飛び地統合の禁止)
    #[test]
    fn test_merge_rejects_non_adjacent_ranges() {
        let mut t = ClusterTopology::single_node(1, "n1");
        let id2 = t.split_range(1, b"m".to_vec()).unwrap(); // [-inf,m) / [m,+inf)
        let id3 = t.split_range(id2, b"z".to_vec()).unwrap(); // [m,z) / [z,+inf)
        // range 1 ([-inf,m)) と id3 ([z,+inf)) は隣接していない (間に[m,z)がある)
        assert!(t.merge_ranges(1, id3).is_none());
        assert_eq!(t.range_count(), 3, "拒否された統合はRange数を変えない");
    }

    /// 存在しないrange_idを指定した場合はNoneを返す(パニックしない)
    #[test]
    fn test_merge_unknown_range_returns_none() {
        let mut t = ClusterTopology::single_node(1, "n1");
        assert!(t.merge_ranges(1, 999).is_none());
    }

    #[test]
    fn test_node_liveness() {
        let mut t = ClusterTopology::single_node(1, "n1");
        t.add_node(2, "n2");
        t.set_node_alive(2, false);
        assert_eq!(t.alive_nodes(), vec![1]);
    }
}
