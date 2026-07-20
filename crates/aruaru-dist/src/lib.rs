//! aruaru-dist: 分散レイヤー (openraft + Range シャーディング)
pub mod dual_database;
pub mod raft;
pub mod shard;
pub mod snapshot_pairing;
#[cfg(feature = "open_raid_z")]
pub mod raid_z_backend;

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
pub use snapshot_pairing::{wire_to_node, InMemorySnapshotBackend, SnapshotBackend, SnapshotPairingRegistry};
pub use dual_database::{DualDatabaseError, DualDatabaseMirror, MirroredMutation, SCHEMA_SQL as DUAL_DATABASE_SCHEMA_SQL};
#[cfg(feature = "open_raid_z")]
pub use raid_z_backend::OpenRaidZSnapshotBackend;
