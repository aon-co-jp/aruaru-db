//! Range シャーディング (CockroachDB 方式)
//!
//! キー空間を Range (デフォルト 64MB) に分割し、128MB 超で自動二分割する。
//! 各 Range は raft グループに対応する。
//!
//! - [`Range`]: キー範囲とレプリカ配置
//! - [`topology`]: クラスタ全体のトポロジとルーティング

pub mod topology;

pub use topology::{ClusterTopology, NodeInfo, RouteTarget};

use serde::{Deserialize, Serialize};

/// Range のデフォルトサイズ目標 (バイト)
pub const DEFAULT_RANGE_SIZE: u64 = 64 * 1024 * 1024;
/// この閾値を超えると自動分割
pub const SPLIT_THRESHOLD: u64 = 128 * 1024 * 1024;

/// キー範囲 [start, end) を表す Range
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    pub range_id: u64,
    /// 開始キー (含む)。None = 最小 (-∞)
    pub start_key: Option<Vec<u8>>,
    /// 終了キー (含まない)。None = 最大 (+∞)
    pub end_key: Option<Vec<u8>>,
    /// この Range を保持するノード ID
    pub replicas: Vec<u64>,
    /// Leader ノード ID
    pub leader: u64,
    pub size_bytes: u64,
}

impl Range {
    /// キーがこの Range に属するか
    pub fn contains(&self, key: &[u8]) -> bool {
        let after_start = match &self.start_key {
            Some(s) => key >= s.as_slice(),
            None => true,
        };
        let before_end = match &self.end_key {
            Some(e) => key < e.as_slice(),
            None => true,
        };
        after_start && before_end
    }

    /// 分割が必要か
    pub fn needs_split(&self) -> bool {
        self.size_bytes > SPLIT_THRESHOLD
    }

    /// 指定キーで 2 つの Range に分割
    pub fn split_at(&self, split_key: Vec<u8>, new_range_id: u64) -> (Range, Range) {
        let left = Range {
            range_id: self.range_id,
            start_key: self.start_key.clone(),
            end_key: Some(split_key.clone()),
            replicas: self.replicas.clone(),
            leader: self.leader,
            size_bytes: self.size_bytes / 2,
        };
        let right = Range {
            range_id: new_range_id,
            start_key: Some(split_key),
            end_key: self.end_key.clone(),
            replicas: self.replicas.clone(),
            leader: self.leader,
            size_bytes: self.size_bytes / 2,
        };
        (left, right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range_contains() {
        let r = Range {
            range_id: 1,
            start_key: Some(b"m".to_vec()),
            end_key: Some(b"z".to_vec()),
            replicas: vec![1, 2, 3],
            leader: 1,
            size_bytes: 0,
        };
        assert!(r.contains(b"n"));
        assert!(!r.contains(b"a"));
        assert!(!r.contains(b"z")); // end は含まない
    }

    #[test]
    fn test_split() {
        let r = Range {
            range_id: 1,
            start_key: None,
            end_key: None,
            replicas: vec![1],
            leader: 1,
            size_bytes: 200 * 1024 * 1024,
        };
        assert!(r.needs_split());
        let (l, rt) = r.split_at(b"m".to_vec(), 2);
        assert_eq!(l.end_key, Some(b"m".to_vec()));
        assert_eq!(rt.start_key, Some(b"m".to_vec()));
        assert_eq!(rt.range_id, 2);
    }
}
