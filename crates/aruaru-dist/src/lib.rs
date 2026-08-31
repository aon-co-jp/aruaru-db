//! aruaru-dist: 分散レイヤー (openraft + Range シャーディング)
pub mod admin_shared;
pub mod closed_ts;
pub mod engine_applier;
pub mod ephemeral;
pub mod keyring;
pub mod dual_database;
#[cfg(feature = "disaster_email_backup")]
pub mod disaster_email_backup;
pub mod multi_raft;
pub mod raft;
pub mod shard;
pub mod snapshot_pairing;
pub mod wal_service;
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
    AppendEntriesReq, AppendEntriesResp, AppendResult, Applier, BinaryTcpTransport,
    HttpSideTransport, HttpTransport, LogEntry, RaftDriver, RaftNode, RaftRole, RaftState,
    RaftWriter, ReplicatedLog, ReplicatedWriter, RequestVoteReq, RequestVoteResp,
    serve_binary_raft, Transport, VoteResult, DEFAULT_COMMIT_TIMEOUT,
};
pub use shard::{
    ClusterStatusSnapshot, ClusterTopology, NodeInfo, NodeStatusSnapshot, Range,
    RangeStatusSnapshot, RouteTarget, DEFAULT_RANGE_SIZE, SPLIT_THRESHOLD,
};
pub use multi_raft::MultiRaftCluster;
pub use engine_applier::EngineApplier;
pub use closed_ts::{
    ClosedTimestampCoordinator, ClosedTimestampTracker, ReadPlan, Timestamp,
    DEFAULT_MAX_STALENESS_NANOS, DEFAULT_TARGET_LAG_NANOS,
};
pub use wal_service::{
    DisaggregatedStorage, Lsn, PageDelta, Pageserver, Safekeeper, Term, WalRecord, WalService,
    WalServiceError, DEFAULT_MAX_REPLICATION_LAG,
};
pub use snapshot_pairing::{wire_to_node, InMemorySnapshotBackend, SnapshotBackend, SnapshotPairingRegistry};
pub use dual_database::{DualDatabaseError, DualDatabaseMirror, MirroredMutation, SCHEMA_SQL as DUAL_DATABASE_SCHEMA_SQL};
#[cfg(feature = "disaster_email_backup")]
pub use disaster_email_backup::{DisasterEmailBackup, DisasterEmailBackupConfig};
#[cfg(feature = "open_raid_z")]
pub use raid_z_backend::OpenRaidZSnapshotBackend;
