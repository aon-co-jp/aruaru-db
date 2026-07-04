//! Raft ノード (状態機械の骨格)
//!
//! 単一プロセス内で Raft の中核ロジックを実装する:
//! - Leader による提案 (propose)
//! - Follower の AppendEntries 受信ロジック (ログ照合 + 競合トランケート)
//! - commit_index の前進と、状態機械への適用パイプライン
//!
//! ネットワーク越しの選挙/複製 RPC は openraft (または shard ルーティング) に委ね、
//! ここではその土台となるログ/適用セマンティクスを提供する。

use parking_lot::RwLock;
use std::collections::HashMap;

use super::command::{Command, CommandResponse};
use super::log::ReplicatedLog;
use super::{LogEntry, RaftRole, RaftState};

/// commit されたコマンドを状態機械へ適用するインタフェース
pub trait Applier: Send + Sync {
    fn apply(&self, command: &Command) -> CommandResponse;
}

/// AppendEntries の結果
#[derive(Debug, Clone, PartialEq)]
pub struct AppendResult {
    pub success: bool,
    /// 受理後の last_index (Leader の nextIndex 調整用)
    pub match_index: u64,
    /// 受信側 term (Leader が下位 term を検知するため)
    pub term: u64,
}

/// RequestVote の結果
#[derive(Debug, Clone, PartialEq)]
pub struct VoteResult {
    pub granted: bool,
    pub term: u64,
}

/// Raft ノード
pub struct RaftNode<A: Applier> {
    state: RwLock<RaftState>,
    log: RwLock<ReplicatedLog>,
    applier: A,
    peers: Vec<u64>,
    /// Leader 用: peer → 複製済み最大インデックス
    match_index: RwLock<HashMap<u64, u64>>,
}

impl<A: Applier> RaftNode<A> {
    pub fn new(node_id: u64, applier: A, peers: Vec<u64>) -> Self {
        Self {
            state: RwLock::new(RaftState::new(node_id)),
            log: RwLock::new(ReplicatedLog::new()),
            applier,
            peers,
            match_index: RwLock::new(HashMap::new()),
        }
    }

    pub fn node_id(&self) -> u64 {
        self.state.read().node_id
    }
    pub fn role(&self) -> RaftRole {
        self.state.read().role
    }
    pub fn term(&self) -> u64 {
        self.state.read().current_term
    }
    pub fn commit_index(&self) -> u64 {
        self.log.read().commit_index()
    }
    pub fn last_index(&self) -> u64 {
        self.log.read().last_index()
    }
    /// 最終ログエントリの term (投票要求用)
    pub fn last_log_term(&self) -> u64 {
        self.log.read().last_term()
    }
    pub fn peers(&self) -> &[u64] {
        &self.peers
    }

    /// peer へ送る AppendEntries の中身を組み立てる
    /// 戻り値: (prev_log_index, prev_log_term, entries, leader_commit)
    pub fn build_append_for(&self, peer: u64) -> (u64, u64, Vec<LogEntry>, u64) {
        let next = self.match_of(peer) + 1;
        let log = self.log.read();
        let prev_index = next.saturating_sub(1);
        let prev_term = log.term_at(prev_index).unwrap_or(0);
        let entries = log.entries_from(next).to_vec();
        (prev_index, prev_term, entries, log.commit_index())
    }

    // ── 役割遷移 ─────────────────────────────────────────────

    pub fn become_leader(&self) {
        let mut s = self.state.write();
        s.role = RaftRole::Leader;
        // Leader 確立時、match_index を初期化
        let mut mi = self.match_index.write();
        mi.clear();
    }
    pub fn become_follower(&self, term: u64) {
        let mut s = self.state.write();
        s.role = RaftRole::Follower;
        if term > s.current_term {
            s.current_term = term;
            s.voted_for = None;
        }
    }
    /// 候補者になり term を上げ、自分自身に投票する
    pub fn become_candidate(&self) -> u64 {
        let mut s = self.state.write();
        s.role = RaftRole::Candidate;
        s.current_term += 1;
        s.voted_for = Some(s.node_id);
        s.current_term
    }

    // ── RequestVote 受信 ─────────────────────────────────────

    /// 候補者からの投票要求を処理する (Raft の選挙安全性)
    pub fn request_vote(
        &self,
        term: u64,
        candidate_id: u64,
        last_log_index: u64,
        last_log_term: u64,
    ) -> VoteResult {
        let mut s = self.state.write();
        // 古い term は拒否
        if term < s.current_term {
            return VoteResult { granted: false, term: s.current_term };
        }
        // 新しい term を見たら Follower 化
        if term > s.current_term {
            s.current_term = term;
            s.voted_for = None;
            s.role = RaftRole::Follower;
        }
        // 候補者ログが自分と同等以上に新しいか
        let (my_last_term, my_last_index) = {
            let log = self.log.read();
            (log.last_term(), log.last_index())
        };
        let log_ok = last_log_term > my_last_term
            || (last_log_term == my_last_term && last_log_index >= my_last_index);
        let can_vote = s.voted_for.is_none() || s.voted_for == Some(candidate_id);

        if can_vote && log_ok {
            s.voted_for = Some(candidate_id);
            VoteResult { granted: true, term: s.current_term }
        } else {
            VoteResult { granted: false, term: s.current_term }
        }
    }

    // ── Leader: 複製進捗と多数決コミット ─────────────────────

    /// AppendEntries 応答を受けて peer の match_index を更新
    pub fn update_match(&self, peer: u64, match_index: u64) {
        self.match_index.write().insert(peer, match_index);
    }

    /// Leader の nextIndex 算出用: peer へ送る prev_log_index
    pub fn match_of(&self, peer: u64) -> u64 {
        self.match_index.read().get(&peer).copied().unwrap_or(0)
    }

    /// 多数決で commit_index を進める。
    /// クラスタ全体 (自分 + peers) の match_index の中央値まで、
    /// かつそのエントリが現在 term のものなら commit する (Raft の安全性)。
    pub fn maybe_commit(&self) {
        if self.role() != RaftRole::Leader {
            return;
        }
        let cur_term = self.term();
        let last = self.last_index();
        // 自分は last まで複製済み
        let mut indices: Vec<u64> = vec![last];
        {
            let mi = self.match_index.read();
            for p in &self.peers {
                indices.push(mi.get(p).copied().unwrap_or(0));
            }
        }
        indices.sort_unstable_by(|a, b| b.cmp(a)); // 降順
        let quorum = indices.len() / 2; // 過半数位置 (0-based)
        let majority_index = indices[quorum];

        // majority_index のエントリが現在 term なら commit
        if majority_index > self.commit_index() {
            let log = self.log.read();
            if log.term_at(majority_index) == Some(cur_term) {
                drop(log);
                self.log.write().commit_to(majority_index);
            }
        }
    }

    // ── Leader: 提案 ─────────────────────────────────────────

    /// Leader として書き込みコマンドをログへ追記し、割り当てインデックスを返す。
    /// 実際の Follower 複製は呼び出し側 (ネットワーク層) が entries_from で送出する。
    pub fn propose(&self, command: &Command) -> Result<u64, String> {
        if self.role() != RaftRole::Leader {
            return Err("not leader".to_string());
        }
        let term = self.term();
        let idx = self.log.write().append(term, command.encode());
        Ok(idx)
    }

    /// 単一ノード構成 (peers 空) では即コミット可能。
    /// 過半数レプリケーションが成立した想定で commit を進める用途にも使う。
    pub fn try_commit_to(&self, index: u64) {
        self.log.write().commit_to(index);
    }

    // ── Follower: AppendEntries 受信 (Raft ログ照合の中核) ──────

    /// Leader からの AppendEntries を処理する。
    /// prev_log_index/prev_log_term が一致しなければ false (Leader が nextIndex を下げる)。
    pub fn append_entries(
        &self,
        leader_term: u64,
        prev_log_index: u64,
        prev_log_term: u64,
        entries: Vec<LogEntry>,
        leader_commit: u64,
    ) -> AppendResult {
        let mut s = self.state.write();

        // 1. 古い term の Leader は拒否
        if leader_term < s.current_term {
            return AppendResult { success: false, match_index: 0, term: s.current_term };
        }
        // 2. より新しい term を見たら Follower 化して term 更新
        if leader_term > s.current_term {
            s.current_term = leader_term;
        }
        s.role = RaftRole::Follower;
        drop(s);

        let mut log = self.log.write();

        // 3. ログ照合: prev_log_index の term が一致するか
        match log.term_at(prev_log_index) {
            Some(t) if t == prev_log_term => {}
            _ => {
                return AppendResult {
                    success: false,
                    match_index: 0,
                    term: self.state.read().current_term,
                };
            }
        }

        // 4. 競合があれば prev 以降を切り詰めてから追記
        if !entries.is_empty() {
            log.truncate_from(prev_log_index + 1);
            for e in entries {
                // term と payload を保ったまま連番で積み直す
                log.append(e.term, e.payload);
            }
        }

        // 5. leader_commit に追従
        if leader_commit > log.commit_index() {
            log.commit_to(leader_commit);
        }

        let match_index = log.last_index();
        AppendResult {
            success: true,
            match_index,
            term: self.state.read().current_term,
        }
    }

    // ── 状態機械への適用 ─────────────────────────────────────

    /// commit 済みで未適用のエントリを順に Applier へ適用し、適用件数を返す。
    pub fn apply_committed(&self) -> usize {
        // 適用対象を取り出す (ロックを跨がないようコピー)
        let pending: Vec<(u64, Command)> = {
            let log = self.log.read();
            log.unapplied()
                .iter()
                .filter_map(|e| Command::decode(&e.payload).map(|c| (e.index, c)))
                .collect()
        };
        let mut applied = 0;
        let mut last = 0;
        for (idx, cmd) in &pending {
            let resp = self.applier.apply(cmd);
            if !resp.ok {
                tracing::warn!(index = idx, msg = %resp.message, "apply failed");
            }
            last = *idx;
            applied += 1;
        }
        if last > 0 {
            self.log.write().set_applied(last);
            let mut s = self.state.write();
            s.applied_index = last;
            s.commit_index = self.log.read().commit_index();
        }
        applied
    }

    /// 現在の Raft 状態スナップショット
    pub fn snapshot_state(&self) -> RaftState {
        let mut st = self.state.read().clone();
        st.commit_index = self.log.read().commit_index();
        st
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    /// 適用された SQL を記録するテスト用 Applier
    struct RecordingApplier {
        applied: Mutex<Vec<String>>,
    }
    impl Applier for RecordingApplier {
        fn apply(&self, command: &Command) -> CommandResponse {
            if let Command::Exec(sql) = command {
                self.applied.lock().push(sql.clone());
            }
            CommandResponse::ok()
        }
    }

    fn node() -> RaftNode<RecordingApplier> {
        RaftNode::new(1, RecordingApplier { applied: Mutex::new(vec![]) }, vec![2, 3])
    }

    #[test]
    fn test_leader_propose_commit_apply() {
        let n = node();
        n.become_leader();
        let i1 = n.propose(&Command::Exec("INSERT 1".into())).unwrap();
        let i2 = n.propose(&Command::Exec("INSERT 2".into())).unwrap();
        assert_eq!((i1, i2), (1, 2));
        // 過半数複製が成立した想定で commit
        n.try_commit_to(2);
        let applied = n.apply_committed();
        assert_eq!(applied, 2);
        // 二重適用されない
        assert_eq!(n.apply_committed(), 0);
    }

    #[test]
    fn test_propose_requires_leader() {
        let n = node();
        assert!(n.propose(&Command::Noop).is_err());
    }

    #[test]
    fn test_append_entries_log_matching() {
        let n = node();
        // 空ログに prev_index=0/term=0 で2件追記
        let entries = vec![
            LogEntry { term: 1, index: 1, payload: Command::Exec("a".into()).encode() },
            LogEntry { term: 1, index: 2, payload: Command::Exec("b".into()).encode() },
        ];
        let r = n.append_entries(1, 0, 0, entries, 2);
        assert!(r.success);
        assert_eq!(r.match_index, 2);
        // commit 追従 → 適用
        assert_eq!(n.apply_committed(), 2);
    }

    #[test]
    fn test_append_entries_rejects_mismatch() {
        let n = node();
        // prev_index=5 は存在しない → 照合失敗
        let r = n.append_entries(1, 5, 1, vec![], 0);
        assert!(!r.success);
    }

    #[test]
    fn test_stale_term_rejected() {
        let n = node();
        n.become_follower(5);
        let r = n.append_entries(3, 0, 0, vec![], 0); // 古い term
        assert!(!r.success);
        assert_eq!(r.term, 5);
    }
}
