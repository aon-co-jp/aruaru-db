//! Raft RPC メッセージ (ノード間通信)
//!
//! AppendEntries / RequestVote を JSON でやり取りする。
//! トランスポート (HTTP 等) はこの型を運ぶだけで、ロジックは RaftNode が持つ。

use serde::{Deserialize, Serialize};

use super::LogEntry;

/// AppendEntries 要求 (Leader → Follower。ハートビート兼ログ複製)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntriesReq {
    pub term: u64,
    pub leader_id: u64,
    pub prev_log_index: u64,
    pub prev_log_term: u64,
    pub entries: Vec<LogEntry>,
    pub leader_commit: u64,
}

/// AppendEntries 応答
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntriesResp {
    pub term: u64,
    pub success: bool,
    /// 受理後の match_index (Leader の進捗追跡用)
    pub match_index: u64,
    /// 応答元ノード
    pub from: u64,
}

/// RequestVote 要求 (Candidate → 他ノード)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestVoteReq {
    pub term: u64,
    pub candidate_id: u64,
    pub last_log_index: u64,
    pub last_log_term: u64,
}

/// RequestVote 応答
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestVoteResp {
    pub term: u64,
    pub vote_granted: bool,
    pub from: u64,
}
