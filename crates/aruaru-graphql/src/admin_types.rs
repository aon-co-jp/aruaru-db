//! 管理操作用の GraphQL 型 (REST 全廃・GraphQL 一本化)
//!
//! すべての管理操作（バックアップ・クラスタ・マイグレーション・
//! レジストリ・並列・フェデレーション）を GraphQL Query/Mutation で公開する。

use async_graphql::SimpleObject;

// ── レジストリ ────────────────────────────────────────────────

#[derive(SimpleObject, Clone)]
pub struct DbEntryGql {
    pub id: String,
    pub name: String,
    pub category: String,
    pub wire: String,
    pub status: String,
    pub rank: Option<i32>,
    pub score: Option<f64>,
    pub updated_at: String,
}

#[derive(SimpleObject, Clone)]
pub struct RegistrySummaryGql {
    pub total: i32,
    pub connectable: i32,
    pub ga: i32,
    pub beta: i32,
    pub pg_compatible: i32,
    pub planned: i32,
}

#[derive(SimpleObject, Clone)]
pub struct CrawlResultGql {
    pub ok: bool,
    pub updated: i32,
    pub message: String,
}

#[derive(SimpleObject, Clone)]
pub struct ConnTestGql {
    pub ok: bool,
    pub message: String,
    pub server_version: Option<String>,
}

// ── バックアップ ───────────────────────────────────────────────

#[derive(SimpleObject, Clone)]
pub struct BackupGql {
    pub id: String,
    pub created_at: String,
    pub branch: String,
    pub commit_id: String,
    pub kind: String,
    pub size_mb: f64,
    pub path: String,
    pub status: String,
}

#[derive(SimpleObject, Clone)]
pub struct ScheduleGql {
    pub enabled: bool,
    pub cron: String,
    pub kind: String,
    pub next_run: Option<String>,
}

// ── クラスタ ───────────────────────────────────────────────────

#[derive(SimpleObject, Clone)]
pub struct NodeStatusGql {
    pub node_id: i64,
    pub addr: String,
    pub role: String,
    pub alive: bool,
    pub commit_index: i64,
    pub applied_index: i64,
    pub ranges: i32,
    pub disk_used_gb: f64,
}

#[derive(SimpleObject, Clone)]
pub struct RangeGql {
    pub range_id: i64,
    pub start_key: String,
    pub end_key: String,
    pub leader_node: i64,
    pub replicas: Vec<i64>,
    pub size_mb: f64,
}

#[derive(SimpleObject, Clone)]
pub struct ClusterStatsGql {
    pub total_nodes: i32,
    pub healthy_nodes: i32,
    pub total_ranges: i32,
    pub total_rows: i64,
    pub table_count: i32,
    pub replication_factor: i32,
    pub under_replicated: Vec<i64>,
}

#[derive(SimpleObject, Clone)]
pub struct ClusterStatusGql {
    pub stats: ClusterStatsGql,
    pub nodes: Vec<NodeStatusGql>,
    pub ranges: Vec<RangeGql>,
}

// ── マイグレーション ───────────────────────────────────────────

#[derive(SimpleObject, Clone)]
pub struct TableInfoGql {
    pub schema: String,
    pub name: String,
    pub estimated_rows: i64,
}

#[derive(SimpleObject, Clone)]
pub struct MigrateResultGql {
    pub success: bool,
    pub wire: String,
    pub total_rows: i64,
    pub commit_id: String,
    pub message: String,
    pub tables: Vec<TableImportGql>,
}

#[derive(SimpleObject, Clone)]
pub struct TableImportGql {
    pub table: String,
    pub rows: Option<i64>,
    pub error: Option<String>,
}

// ── 並列実行 ──────────────────────────────────────────────────

#[derive(SimpleObject, Clone)]
pub struct ParallelConfigGql {
    pub enabled: bool,
    pub max_workers: i32,
    pub chunk_size: i32,
    pub strategy: String,
}

#[derive(SimpleObject, Clone)]
pub struct ExplainStepGql {
    pub step: i32,
    pub node: String,
    pub range: String,
    pub operation: String,
    pub estimated_rows: i64,
}

#[derive(SimpleObject, Clone)]
pub struct ParallelJobGql {
    pub job_id: String,
    pub sql: String,
    pub status: String,
    pub workers: i32,
    pub elapsed_ms: i64,
    pub rows_processed: i64,
    pub started_at: String,
}

// ── APIキー自動ライフサイクル管理(2026-08-29(続き)新設) ────────

#[derive(SimpleObject, Clone)]
pub struct KeyStatusGql {
    pub issued_key_count: i32,
}

#[derive(SimpleObject, Clone)]
pub struct KeyRevokeResultGql {
    pub revoked_count: i32,
}

// ── オブジェクトテーブル(Databend方式・時間旅行、2026-08-29(続き3)新設) ──
//
// REST `GET /admin/object-table` と同一の実データ(`aruaru-backup::
// table_format::ObjectTable` のスナップショット連鎖)をGraphQLへ写す。
// スナップショット連鎖=時間旅行=VersionlessAPI互換の実体なので、
// aruaru-db + RPoem SET としての価値を直接強化する移行対象として選定。

#[derive(SimpleObject, Clone)]
pub struct ObjectTableSnapshotGql {
    pub snapshot_id: String,
    /// 直前スナップショットID(時間旅行の連鎖、根は None)。
    pub prev_snapshot_id: Option<String>,
    pub timestamp: i64,
    pub segments: Vec<String>,
    pub row_count: i64,
}

#[derive(SimpleObject, Clone)]
pub struct ObjectTableStatusGql {
    pub table_key: String,
    /// 現在のスナップショット(未コミット時は None)。
    pub current: Option<ObjectTableSnapshotGql>,
    pub history_len: i32,
    pub history: Vec<ObjectTableSnapshotGql>,
}

#[derive(SimpleObject, Clone)]
pub struct ObjectTableCommitResultGql {
    pub snapshot_id: String,
    pub block_count: i32,
}

#[derive(SimpleObject, Clone)]
pub struct ObjectTablePruneResultGql {
    pub snapshot_id: String,
    /// `"equality"`(bloom filter)/ `"range"`(min/max 統計)。
    pub predicate: String,
    pub column: String,
    pub kept_blocks: i32,
    /// range述語のみ。equalityでは 0。
    pub skipped_segments: i32,
    /// range述語のみ。equalityでは 0。
    pub skipped_blocks: i32,
    pub locations: Vec<String>,
}

// ── フェデレーション ───────────────────────────────────────────

#[derive(SimpleObject, Clone)]
pub struct FederatedSourceGql {
    pub name: String,
    pub kind: String,
    pub uri: String,
    pub status: String,
    pub tables: i32,
}
