//! aruaru-dist: 分散レイヤー (openraft + Range シャーディング)
pub mod raft;
pub mod shard;

/// ノード設定
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeConfig {
    pub node_id: u64,
    pub bind_addr: String,
    pub peers: Vec<PeerConfig>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PeerConfig {
    pub node_id: u64,
    pub addr: String,
}

pub use raft::{
    command::{Command, CommandResponse},
    AppendEntriesReq, AppendEntriesResp, AppendResult, Applier, HttpTransport, LogEntry,
    RaftDriver, RaftNode, RaftRole, RaftState, RaftWriter, ReplicatedLog, ReplicatedWriter,
    RequestVoteReq, RequestVoteResp, Transport, VoteResult, DEFAULT_COMMIT_TIMEOUT,
};
pub use shard::{
    ClusterTopology, NodeInfo, Range, RouteTarget, DEFAULT_RANGE_SIZE, SPLIT_THRESHOLD,
};
