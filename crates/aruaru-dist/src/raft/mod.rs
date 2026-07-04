//! Raft 合意レイヤー
//!
//! 各 Range が独立した Raft グループを持ち、Leader が書き込みを受け付け、
//! Follower がログをレプリケートする (CockroachDB 方式)。
//!
//! ## 構成
//! - [`command`]: 複製する書き込みコマンド (Command / CommandResponse)
//! - [`log`]: 複製ログ (追記・ログ照合・コミット・トランケート)
//! - [`node`]: Raft ノード (提案・AppendEntries 受信・適用パイプライン)
//!
//! ## openraft 統合シーム (次段階)
//! 本モジュールは Raft のログ/適用セマンティクスを自前実装で提供する。
//! 本番のリーダー選挙・ハートビート・スナップショット・ネットワーク RPC は
//! `openraft` に委譲する計画で、その際の対応は以下:
//! - openraft の `D` (AppData) = [`command::Command`]
//! - openraft の `R` (AppDataResponse) = [`command::CommandResponse`]
//! - `RaftLogStorage` 実装は [`log::ReplicatedLog`] を内部に持つ
//! - `RaftStateMachine` 実装は [`node::Applier`] 経由で QueryEngine へ適用
//! - `RaftNetwork` 実装が AppendEntries を [`node::RaftNode::append_entries`] に橋渡し
//!
//! この分離により、合意エンジンを openraft に差し替えてもログ/適用ロジックは再利用できる。

pub mod command;
pub mod driver;
pub mod log;
pub mod node;
pub mod rpc;
pub mod transport;
pub mod writer;

pub use command::{Command, CommandResponse};
pub use driver::RaftDriver;
pub use log::ReplicatedLog;
pub use node::{AppendResult, Applier, RaftNode, VoteResult};
pub use rpc::{AppendEntriesReq, AppendEntriesResp, RequestVoteReq, RequestVoteResp};
pub use transport::{HttpTransport, Transport};
pub use writer::{RaftWriter, ReplicatedWriter, DEFAULT_COMMIT_TIMEOUT};

use serde::{Deserialize, Serialize};

/// Raft ノードの役割
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RaftRole {
    Leader,
    Follower,
    Candidate,
    Learner,
}

/// Raft ログエントリ (= 1 つの書き込み操作)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub term: u64,
    pub index: u64,
    /// シリアライズされた書き込み操作 (例: INSERT/UPDATE/DELETE)
    pub payload: Vec<u8>,
}

/// Raft グループの状態スナップショット
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftState {
    pub node_id: u64,
    pub role: RaftRole,
    pub current_term: u64,
    pub commit_index: u64,
    pub applied_index: u64,
    /// 現在の term で投票した候補 (なければ None)
    pub voted_for: Option<u64>,
}

impl RaftState {
    pub fn new(node_id: u64) -> Self {
        Self {
            node_id,
            role: RaftRole::Follower,
            current_term: 0,
            commit_index: 0,
            applied_index: 0,
            voted_for: None,
        }
    }
}
