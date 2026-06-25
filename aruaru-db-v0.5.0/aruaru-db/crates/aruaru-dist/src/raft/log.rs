//! Raft 複製ログ
//!
//! 1-based の連続インデックスでエントリを保持する。
//! Raft の中核である「ログ照合 (Log Matching)」「コミット」「競合トランケート」
//! を実装する。ネットワーク (AppendEntries RPC) は上位 (node) が駆動する。

use super::LogEntry;

/// 複製ログ
#[derive(Debug, Default)]
pub struct ReplicatedLog {
    /// entries[i].index == i + 1 (1-based)
    entries: Vec<LogEntry>,
    /// コミット済みの最大インデックス (0 = 無し)
    commit_index: u64,
    /// 状態機械へ適用済みの最大インデックス
    last_applied: u64,
}

impl ReplicatedLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn commit_index(&self) -> u64 {
        self.commit_index
    }
    pub fn last_applied(&self) -> u64 {
        self.last_applied
    }

    /// 最後のエントリのインデックス (空なら 0)
    pub fn last_index(&self) -> u64 {
        self.entries.last().map(|e| e.index).unwrap_or(0)
    }

    /// 最後のエントリの term (空なら 0)
    pub fn last_term(&self) -> u64 {
        self.entries.last().map(|e| e.term).unwrap_or(0)
    }

    /// 指定インデックスの term
    pub fn term_at(&self, index: u64) -> Option<u64> {
        if index == 0 {
            return Some(0); // 番兵 (prev_index=0 の照合用)
        }
        self.entries.get((index - 1) as usize).map(|e| e.term)
    }

    /// Leader 側: 新しい payload を現在 term で追記し、割り当てたインデックスを返す
    pub fn append(&mut self, term: u64, payload: Vec<u8>) -> u64 {
        let index = self.last_index() + 1;
        self.entries.push(LogEntry { term, index, payload });
        index
    }

    /// index 以降 (index 含む) のエントリを削除 (競合解決)
    pub fn truncate_from(&mut self, index: u64) {
        if index == 0 {
            self.entries.clear();
        } else {
            self.entries.truncate((index - 1) as usize);
        }
        // コミット済みより前に切り詰めることは Raft 上は起こらない想定だが安全側に
        if self.commit_index > self.last_index() {
            self.commit_index = self.last_index();
        }
    }

    /// commit_index を index まで進める (last_index を超えない)
    pub fn commit_to(&mut self, index: u64) {
        let target = index.min(self.last_index());
        if target > self.commit_index {
            self.commit_index = target;
        }
    }

    /// last_applied を更新 (apply 後に呼ぶ)
    pub fn set_applied(&mut self, index: u64) {
        if index > self.last_applied {
            self.last_applied = index;
        }
    }

    /// commit 済みだが未適用のエントリ (last_applied+1 ..= commit_index)
    pub fn unapplied(&self) -> &[LogEntry] {
        let from = self.last_applied as usize; // 0-based の開始
        let to = self.commit_index as usize;
        if to > from && to <= self.entries.len() {
            &self.entries[from..to]
        } else {
            &[]
        }
    }

    /// index 以降のエントリ (レプリケーション送出用)
    pub fn entries_from(&self, index: u64) -> &[LogEntry] {
        if index == 0 {
            return &self.entries;
        }
        let start = (index - 1) as usize;
        if start < self.entries.len() {
            &self.entries[start..]
        } else {
            &[]
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_append_and_index() {
        let mut log = ReplicatedLog::new();
        assert_eq!(log.last_index(), 0);
        assert_eq!(log.append(1, b"a".to_vec()), 1);
        assert_eq!(log.append(1, b"b".to_vec()), 2);
        assert_eq!(log.last_index(), 2);
        assert_eq!(log.last_term(), 1);
        assert_eq!(log.term_at(1), Some(1));
        assert_eq!(log.term_at(0), Some(0));
        assert_eq!(log.term_at(99), None);
    }

    #[test]
    fn test_truncate() {
        let mut log = ReplicatedLog::new();
        log.append(1, b"a".to_vec());
        log.append(1, b"b".to_vec());
        log.append(2, b"c".to_vec());
        log.truncate_from(2); // index 2,3 を削除
        assert_eq!(log.last_index(), 1);
    }

    #[test]
    fn test_commit_and_unapplied() {
        let mut log = ReplicatedLog::new();
        log.append(1, b"a".to_vec());
        log.append(1, b"b".to_vec());
        log.append(1, b"c".to_vec());
        log.commit_to(2);
        assert_eq!(log.commit_index(), 2);
        // 未適用 = index 1,2
        assert_eq!(log.unapplied().len(), 2);
        log.set_applied(2);
        assert_eq!(log.unapplied().len(), 0);
        // commit はログ長を超えない
        log.commit_to(99);
        assert_eq!(log.commit_index(), 3);
    }
}
