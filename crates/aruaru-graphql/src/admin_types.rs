//! 管理操作用の GraphQL 型 (REST 全廃・GraphQL 一本化)
//!
//! すべての管理操作（バックアップ・クラスタ・マイグレーション・
//! レジストリ・並列・フェデレーション）を GraphQL Query/Mutation で公開する。

use async_graphql::SimpleObject;

use crate::QueryResultGql;

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

// ── Closed timestamp / Follower read(CockroachDB 方式、2026-08-29 再設計 P3) ──
//
// 旧 REST `GET /admin/closed-timestamp`・`POST /admin/closed-timestamp/{range,
// advance,plan}` の等価。observability(status / plan)は `AdminQuery`、
// ワンショット操作(register / advance)は `AdminMutation` へ(§2.2 判断フロー)。
// タイムスタンプ・LSN はすべて u64 論理ナノ秒/位置であり、GraphQL の Int
// (=JSON number、f64 精度)では 2^53 を超えると欠落するため **String 表現**
// で受け渡す(revival メッセージの明示指示)。`range_id` は小さな整数なので
// Int(i64)のまま。

#[derive(SimpleObject, Clone)]
pub struct ClosedTimestampRangeGql {
    pub range_id: i64,
    /// u64 論理ナノ秒(精度保持のため String)。
    pub closed_timestamp: String,
    /// 進行中書き込みの最小時刻(無ければ null)。
    pub lowest_in_flight: Option<String>,
    pub target_lag_nanos: String,
}

#[derive(SimpleObject, Clone)]
pub struct ClosedTimestampStatusGql {
    pub range_count: i32,
    pub ranges: Vec<ClosedTimestampRangeGql>,
}

#[derive(SimpleObject, Clone)]
pub struct ClosedTsRegisterResultGql {
    pub range_id: i64,
    pub closed_timestamp: String,
}

#[derive(SimpleObject, Clone)]
pub struct ClosedTsAdvanceEntryGql {
    pub range_id: i64,
    pub closed_timestamp: String,
}

/// `table` を指定して follower read が許可された場合の実データ読み出し結果
/// (`QueryEngine::select_follower_read` = `AS OF COMMIT` と同じ Prolly Tree
/// 経由の読み取り)。読み取り自体がエラーになった場合は `ok=false` +
/// `error`(GraphQL 全体をエラーにはしない——プラン判定自体は成功している)。
#[derive(SimpleObject, Clone)]
pub struct FollowerReadDataGql {
    pub ok: bool,
    pub error: Option<String>,
    pub result: Option<QueryResultGql>,
}

#[derive(SimpleObject, Clone)]
pub struct FollowerReadPlanGql {
    /// `"follower_read"` | `"route_to_leaseholder"`。
    pub plan: String,
    pub is_follower_read: bool,
    /// follower read が許可されたときの読み取り時刻(u64 → String)。
    pub read_timestamp: Option<String>,
    pub staleness_nanos: Option<String>,
    /// leaseholder へルーティングする場合の理由。
    pub reason: Option<String>,
    /// `table` 指定時のみ。
    pub data: Option<FollowerReadDataGql>,
}

// ── WAL サービス(Neon 方式 safekeeper/pageserver 分離、2026-08-29 再設計 P3) ──
//
// 旧 REST `GET /admin/wal-service`・`POST /admin/wal-service/{append,page,
// image-layer}` の等価。status / page(`get_page_at_lsn` = 純粋な読み取り)は
// `AdminQuery`、append(耐久化)/ image-layer(compaction)は `AdminMutation`。
// LSN・term は u64 のため String 表現。

#[derive(SimpleObject, Clone)]
pub struct WalSafekeeperGql {
    pub id: i64,
    pub accepted_term: String,
    pub flush_lsn: String,
}

#[derive(SimpleObject, Clone)]
pub struct WalPageserverGql {
    pub last_record_lsn: String,
    pub max_replication_lag: String,
    pub page_keys: Vec<String>,
}

#[derive(SimpleObject, Clone)]
pub struct WalServiceStatusGql {
    pub term: String,
    pub quorum: i32,
    pub commit_lsn: String,
    pub safekeepers: Vec<WalSafekeeperGql>,
    pub pageserver: WalPageserverGql,
}

#[derive(SimpleObject, Clone)]
pub struct WalAppendResultGql {
    pub commit_lsn: String,
    pub applied_lsn: String,
    pub record_count: i32,
}

#[derive(SimpleObject, Clone)]
pub struct WalPageGql {
    pub page_key: String,
    pub lsn: String,
    pub len: i32,
    pub data: String,
    pub image_layer_lsn: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct WalImageLayerResultGql {
    pub page_key: String,
    pub gc_cutoff_lsn: String,
    pub dropped_deltas: i32,
}

// ── ScyllaDB shard-per-core ストア(2026-08-29 再設計 P3) ──
//
// 旧 REST `POST /admin/sharded-store`・`GET /admin/sharded-store/:key`・
// `GET /admin/sharded-store-stats` の等価。put は `AdminMutation`、
// get / stats は `AdminQuery`。

#[derive(SimpleObject, Clone)]
pub struct ShardedStorePutResultGql {
    pub key: String,
    pub shard_id: i64,
}

#[derive(SimpleObject, Clone)]
pub struct ShardedStoreEntryGql {
    pub key: String,
    pub shard_id: i64,
    pub found: bool,
    pub value: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct ShardedStoreStatsGql {
    pub shard_count: i32,
    pub per_shard_len: Vec<i64>,
    pub total_len: i64,
}

// ── Vitess Reshard(併合)+ VTGate scatter-gather(2026-08-31、REST撤廃) ──
// 旧 REST `POST /admin/multi-raft/split`・`/merge`・
// `GET /admin/multi-raft/scatter-query` の等価。

#[derive(SimpleObject, Clone)]
pub struct MultiRaftSplitResultGql {
    pub success: bool,
    pub new_range_id: Option<i64>,
    pub range_count: i32,
    pub message: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct MultiRaftMergeResultGql {
    pub success: bool,
    pub merged_range_id: Option<i64>,
    pub range_count: i32,
    pub message: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct MultiRaftRangeReadingGql {
    pub range_id: i64,
    pub commit_index: i64,
    pub role: String,
}

// ── ephemeral SQL pod(2026-08-31、trait注入リファクタでREST撤廃) ──
// 旧 REST `POST /admin/ephemeral-query` の等価。`Mutation.ephemeralQuery`。

#[derive(SimpleObject, Clone)]
pub struct EphemeralQueryResultGql {
    pub success: bool,
    pub tenant_id: String,
    pub result: Option<QueryResultGql>,
    pub error: Option<String>,
    /// ワーカー起動自体が失敗した場合(current_exe解決失敗・プロセス
    /// spawn失敗等)のメッセージ。`result`/`error`はこの場合`None`のまま。
    pub message: Option<String>,
}

// ── HTAP 列レプリカ観測(2026-09-02 続き23、Query.htapReplicas) ──
// TiFlash `INFORMATION_SCHEMA.TIFLASH_REPLICA`(PROGRESS/AVAILABLE)相当。
// 同居 `ColumnarApplier`(`aruaru.yaml: htap.columnar_replicas: true`)の
// 行→列非同期変換レプリカの状態を、GraphQL 単一サーフェスから観測する。

#[derive(SimpleObject, Clone)]
pub struct HtapReplicaStatusGql {
    pub table: String,
    /// 一度でもレプリケートされていれば true(TiFlash AVAILABLE 相当)。
    pub available: bool,
    /// 同期進捗 0.0〜1.0(列レプリカ実効行数 ÷ 行ストア行数、TiFlash PROGRESS 相当)。
    pub progress: f64,
    /// MoR ビューの block 数(base + delta)。
    pub columnar_block_count: i32,
    /// deletion vector 差し引き後の実効行数。
    pub columnar_live_row_count: i64,
    /// deletion vector にマークされた行位置の総数(論理削除数)。
    pub deletion_vector_positions: i64,
    /// 適用済みの最大 Raft ログインデックス(同居モードでは 0)。
    pub applied_index: i64,
    /// 適用済み `Command::Commit` 通し番号(MVCC SI ゲート用)。
    pub applied_commit_seq: i64,
    /// 累計レプリケーション回数。
    pub replication_count: i64,
    /// 枝刈り込みプレビュー(クエリ引数 pruneColumn を渡した場合のみ)。
    pub prune: Option<HtapPrunePreviewGql>,
}

#[derive(SimpleObject, Clone)]
pub struct HtapPrunePreviewGql {
    pub column: String,
    pub op: String,
    pub value: String,
    pub total_blocks: i32,
    pub kept_blocks: i32,
    pub skipped_blocks: i32,
    pub kept_live_rows: i64,
}

// ── HLC 観測(2026-09-03 続き26、P-HLC-3b、Query.hlcNow) ──
// 案A(フル精度 2 フィールド)へ移行した HLC の現在値を観測する。
// `closedTsAdvance` 等が受け渡す u64 ordinal(案B 射影)も併記。

#[derive(SimpleObject, Clone)]
pub struct HlcNowGql {
    /// フル精度 Unix エポックからのナノ秒(u64 のため String)。
    pub wall_nanos: String,
    /// 論理カウンタ。
    pub logical: i32,
    /// wall_nanos が実際の物理クロック読み値より先行しているか。
    pub synthetic: bool,
    /// `closed_ts` 等が受け取る u64 ordinal(= `wall_nanos + logical`、
    /// P-HLC-3d でシフト・マスク撤去済み、フル精度 Unix ナノ秒スケール、String)。
    pub ordinal: String,
    /// 設定中のクロックスキュー上限(ミリ秒、0 = 無効)。
    pub max_offset_ms: i64,
    /// uncertainty interval の上端 wall_nanos + max_offset(String)。
    pub uncertainty_upper_nanos: String,
}
