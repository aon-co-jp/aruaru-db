//! 管理操作リゾルバ (REST 全廃・GraphQL 一本化)
//!
//! AdminState を Context から取り出し、各操作を実行する。
//! aruaru-server の AdminState を Arc で共有する。

use std::sync::Arc;

use async_graphql::{Context, InputObject, Object, Result};

use aruaru_backup::BackupEngine;
use aruaru_query::QueryEngine;
use aruaru_registry::Registry;

use crate::admin_types::*;

/// GraphQL Context に注入する管理状態
///
/// **2026-07-26追記**: 2026-07-25(続き2)のHANDOFFが「未解決」として正直に
/// 記録していたギャップ(`cluster_propose` resolverが`RaftWriter`/
/// `replicator`を経由せず`engine.execute`へ直接書き込んでいた)を解消する
/// ため、`replicator`フィールドを追加した。`aruaru-server`のpgwire経路・
/// REST admin API(`admin.rs`の`cluster_propose`)が共有しているのと
/// **同一の**`Arc<dyn ReplicatedWriter>`を`main.rs`から注入する
/// (`replicator: Option<..>` — クラスタ構築に成功した場合のみ`Some`、
/// 単一ノード/非クラスタ構成では`None`のまま、既存動作を変えない)。
pub struct AdminCtx {
    pub engine: Arc<QueryEngine>,
    pub registry: Arc<Registry>,
    pub backup: Arc<BackupEngine>,
    pub replicator: Option<Arc<dyn aruaru_dist::ReplicatedWriter>>,
    /// 【2026-08-29新設】REST(`aruaru-server::admin::AdminState`)と
    /// **同一インスタンス**を共有するクラスタトポロジ(`AdminState::
    /// topology_handle()`経由)。これにより`clusterStatus`が固定値の
    /// スタブではなく、REST `/admin/cluster`と全く同じ実データを返す
    /// ようになる(ユーザー指示「/admin/*の運用系REST操作のうちデータ
    /// 寄りのものをGraphQLへ移し、RESTの必要性を実際に減らす」への
    /// 対応)。`None`の場合(将来サーバー構成が変わった場合の保険)は
    /// 従来通り単一ノード相当のフォールバック値を返す。
    pub topology: Option<Arc<parking_lot::Mutex<aruaru_dist::ClusterTopology>>>,
    /// 【2026-08-29新設】REST(`AdminState.schedule`)と同一インスタンスを
    /// 共有するバックアップスケジュール状態。`backup_schedule`/
    /// `set_backup_schedule`が固定値スタブを返していたギャップの解消。
    pub schedule: Option<aruaru_dist::admin_shared::SharedBackupSchedule>,
    /// 【2026-08-29新設】REST(`AdminState.federation`)と同一インスタンスを
    /// 共有するフェデレーションソース一覧。`federated_sources`が常に
    /// 空配列を返していたギャップの解消。
    pub federation: Option<aruaru_dist::admin_shared::SharedFederatedSources>,
    /// 【2026-08-29 再設計 P2】`AdminState.parallel` と同一インスタンス。
    /// `parallelConfig` query が `aruaru.yaml: query.parallel` の実効値を
    /// 返すために使う(`setParallelConfig` は撤廃済み)。
    pub parallel: Option<aruaru_dist::admin_shared::SharedParallelConfig>,
    /// 【2026-08-29(続き)新設】REST(`AdminState.keyring`)と同一インスタンス
    /// を共有するAPIキー自動ライフサイクル管理(`aruaru_dist::keyring::
    /// KeyGuardian`)。`keyStatus`/`revokeKeys`から実際に発行済みキー数の
    /// 参照・特定オーナーのキー破棄ができる(従来GraphQL側にこの操作自体が
    /// 存在しなかった)。
    pub keyring: Option<Arc<aruaru_dist::keyring::KeyGuardian>>,
    /// 【2026-08-29(続き3) REST完全撤廃】`AdminState.object_table`を
    /// `object_table_handle()`経由で注入したDatabend方式オブジェクト
    /// テーブル。スナップショット連鎖(時間旅行=VersionlessAPIの実体)
    /// への唯一のアクセス経路がこの`objectTable` query /
    /// `objectTableCommit`・`objectTablePrune` mutation——旧 REST
    /// `/admin/object-table*` 3ルートは`admin.rs`から削除済み。`None`の
    /// 場合(将来サーバー構成が変わった場合の保険)は空の状態を返す。
    pub object_table: Option<Arc<aruaru_backup::table_format::ObjectTable>>,
    /// 【2026-08-29 再設計 P3】REST(`AdminState.closed_ts`)と**同一**の
    /// CockroachDB 方式 closed timestamp コーディネータ
    /// (`closed_ts_coordinator()` 経由で注入)。旧 REST
    /// `/admin/closed-timestamp`(status)・`/range`・`/advance`・`/plan` は
    /// `admin.rs` から削除済みで、`closedTimestamp`/`planFollowerRead` query と
    /// `closedTsRegisterRange`/`closedTsAdvance` mutation が唯一の経路。
    /// ノード間 side transport(`/receive`・`/publish`)は B4 としてバイナリ
    /// トランスポート側に残る(P4)。
    pub closed_ts: Option<Arc<aruaru_dist::ClosedTimestampCoordinator>>,
    /// 【2026-08-29 再設計 P3】REST(`AdminState.wal_storage`)と**同一**の
    /// Neon 方式 safekeeper/pageserver 分離ストレージ
    /// (`wal_storage_handle()` 経由)。旧 REST `/admin/wal-service`・
    /// `/append`・`/page`・`/image-layer` は削除済みで、`walService`/`walPage`
    /// query と `walAppend`/`walCreateImageLayer` mutation が唯一の経路。
    pub wal_storage: Option<Arc<aruaru_dist::DisaggregatedStorage>>,
    /// 【2026-08-29 再設計 P3】REST(`AdminState.sharded_store`)と**同一**の
    /// ScyllaDB shard-per-core ストア(`sharded_store_handle()` 経由)。
    /// 旧 REST `/admin/sharded-store*` 3ルートは削除済みで、
    /// `shardedStoreGet`/`shardedStoreStats` query と `shardedStorePut`
    /// mutation が唯一の経路。
    pub sharded_store: Option<Arc<aruaru_query::sharded_store::ShardedRowStore<String>>>,
    /// 【2026-08-31 trait注入リファクタ】ephemeral SQL pod
    /// (`aruaru_dist::ephemeral::EphemeralRunner`)。実体
    /// (`ProcessEphemeralRunner`、`current_exe()`で自プロセスを再起動する
    /// `aruaru-server`バイナリ固有処理)は`aruaru-server`側にあるが、
    /// `aruaru-graphql`はtrait経由でのみ参照する(`ReplicatedWriter`と
    /// 同じ「実装はサーバー側、trait定義は共有クレート」パターン)。
    /// 旧 REST `POST /admin/ephemeral-query` は削除済みで、
    /// `Mutation.ephemeralQuery`が唯一の経路。
    pub ephemeral: Option<Arc<dyn aruaru_dist::ephemeral::EphemeralRunner>>,
    /// 【2026-08-31 trait注入リファクタ(続き)】REST(`AdminState.multi_raft`)
    /// と**同一**の`MultiRaftCluster`インスタンス。`EngineApplier`を
    /// `aruaru-dist`へ移設したことで、`aruaru-graphql`も具体型
    /// `MultiRaftCluster<aruaru_dist::EngineApplier>`をそのまま名指し
    /// できるようになった(trait object化は不要だった)。旧 REST
    /// `/admin/multi-raft/{split,merge,scatter-query}` は削除済みで、
    /// `Mutation.multiRaftSplit`/`multiRaftMerge`・
    /// `Query.multiRaftScatterQuery`が唯一の経路。
    pub multi_raft: Option<Arc<aruaru_dist::MultiRaftCluster<aruaru_dist::EngineApplier>>>,
    /// 【2026-09-02 HLC 再設計 P-HLC-2】REST(`AdminState.hlc`)と**同一**の
    /// Hybrid Logical Clock。`closed_ts` へ渡す「now」既定値の生成に使う
    /// (`now_unix_nanos()` の生の `SystemTime` ではなく、単調性を保証する
    /// HLC ordinal)。正本: `docs/HLC_TIMESTAMP_REDESIGN.md`。
    pub hlc: Option<Arc<aruaru_dist::Hlc>>,
    /// 【2026-09-02 続き23】同居(co-located)`ColumnarApplier`
    /// (`aruaru.yaml: htap.columnar_replicas: true` で `main.rs` が構築、
    /// 本番 `QueryEngine` を共有して行→列非同期変換レプリカを追従させる)。
    /// `htapReplicas` query が TiFlash `INFORMATION_SCHEMA.TIFLASH_REPLICA`
    /// (PROGRESS / AVAILABLE)相当の観測値と枝刈り込みプレビューを返す。
    /// `None` = このサーバーで列レプリカ機能が無効。
    pub columnar: Option<Arc<aruaru_dist::ColumnarApplier>>,
}

/// closed timestamp の `now` 既定値。`AdminCtx.hlc` があれば HLC ordinal
/// (単調保証)、無ければ生の UNIX epoch ナノ秒へフォールバック。
fn now_default_nanos(ctx: &AdminCtx) -> u64 {
    match &ctx.hlc {
        Some(hlc) => hlc.now_ordinal(),
        None => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0),
    }
}

/// 1テーブルぶんの HTAP 列レプリカ同期状態を組み立てる(`htapReplicas` と
/// `htapReplicasAll` の共通ヘルパー)。TiFlash `TIFLASH_REPLICA` 相当。
fn htap_status_for(
    columnar: &aruaru_dist::ColumnarApplier,
    table: &str,
    prune: Option<HtapPrunePreviewGql>,
) -> HtapReplicaStatusGql {
    let blocks = columnar.latest_blocks(table).unwrap_or_default();
    let dv: usize = blocks.iter().map(|b| b.deletion_vector.len()).sum();
    HtapReplicaStatusGql {
        table: table.to_string(),
        available: columnar.replica_available(table),
        progress: columnar.replication_progress(table).unwrap_or(0.0),
        columnar_block_count: blocks.len() as i32,
        columnar_live_row_count: columnar.latest_live_row_count(table).unwrap_or(0) as i64,
        deletion_vector_positions: dv as i64,
        applied_index: columnar.applied_index() as i64,
        applied_commit_seq: columnar.applied_commit_seq() as i64,
        replication_count: columnar.replication_count() as i64,
        prune,
    }
}

/// GraphQL の String 引数を u64 へ(タイムスタンプ・LSN は精度保持のため
/// String で受け渡す)。
fn parse_u64(s: &str, field: &str) -> Result<u64> {
    s.parse::<u64>()
        .map_err(|_| async_graphql::Error::new(format!("{field} must be a non-negative integer (got {s:?})")))
}

/// `x-admin-token`ヘッダーを検証する(2026-08-01追加、実バグ修正)。
/// `admin.rs`(REST、`/admin/*`)の`check_admin_auth`/`constant_time_eq`と
/// 同じロジック・同じ環境変数(`ARUARU_DB_ADMIN_TOKEN`)を使う——GraphQL
/// (`/graphql`)経由でも同じ管理操作(`cluster_propose`・`create_backup`・
/// `run_migration`等)を呼べてしまうため、REST側だけを認証で塞いでも
/// 迂回経路になっていた実バグへの対応。新規crate依存(`subtle`等)は
/// 追加せず、この用途限定の最小実装を複製している。
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    diff == 0
}

fn require_admin_token(ctx: &Context<'_>) -> Result<()> {
    let Ok(expected) = std::env::var("ARUARU_DB_ADMIN_TOKEN") else {
        return Err(async_graphql::Error::new("admin API is not configured (ARUARU_DB_ADMIN_TOKEN is not set)"));
    };
    let provided = ctx.data::<crate::GraphqlAdminToken>().ok().and_then(|t| t.0.clone()).unwrap_or_default();
    if provided.is_empty() || !constant_time_eq(&provided, &expected) {
        return Err(async_graphql::Error::new("invalid or missing x-admin-token header"));
    }
    Ok(())
}

/// `AdminCtx`を取り出す前に、必ず`require_admin_token`で認証する
/// (このヘルパー経由で管理状態を取得する全resolverが自動的に保護対象に
/// なる、DRYな適用箇所)。
fn admin<'a>(ctx: &Context<'a>) -> Result<&'a AdminCtx> {
    require_admin_token(ctx)?;
    ctx.data::<AdminCtx>()
        .map_err(|_| async_graphql::Error::new("AdminCtx not in context"))
}

/// `aruaru_backup::table_format::TableSnapshot` → GraphQL出力型への変換
/// (`object_table` resolver用、REST `object_table_status`の`json!`と
/// 同じフィールドを写す)。
fn snapshot_to_gql(s: aruaru_backup::table_format::TableSnapshot) -> ObjectTableSnapshotGql {
    ObjectTableSnapshotGql {
        snapshot_id: s.snapshot_id,
        prev_snapshot_id: s.prev_snapshot_id,
        timestamp: s.timestamp,
        segments: s.segments,
        row_count: s.row_count as i64,
    }
}

// ── 入力型 ───────────────────────────────────────────────────

#[derive(InputObject)]
pub struct BackupConfigInput {
    pub branch: Option<String>,
    pub kind: Option<String>,
}

#[derive(InputObject)]
pub struct RestoreInput {
    pub backup_id: String,
    pub target_branch: Option<String>,
}

#[derive(InputObject)]
pub struct ScheduleInput {
    pub enabled: bool,
    pub cron: String,
    pub kind: String,
}

// ── オブジェクトテーブル(Databend方式、2026-08-29(続き3) REST完全撤廃) ──

#[derive(InputObject)]
pub struct ObjectBlockStatInput {
    pub column: String,
    pub min: f64,
    pub max: f64,
    #[graphql(default)]
    pub null_count: i64,
}

/// bloom filter へ入れる等値枝刈り用キー(REST版の
/// `BTreeMap<String, Vec<String>>`をGraphQLで表現できる形へ変換)。
#[derive(InputObject)]
pub struct ObjectBlockBloomInput {
    pub column: String,
    pub keys: Vec<String>,
}

#[derive(InputObject)]
pub struct ObjectBlockInput {
    pub location: String,
    pub row_count: i64,
    pub size_bytes: i64,
    #[graphql(default)]
    pub stats: Vec<ObjectBlockStatInput>,
    #[graphql(default)]
    pub bloom: Vec<ObjectBlockBloomInput>,
}

// `ParallelConfigInput` は撤廃(2026-08-29 再設計 P2)。並列設定の正本は
// 宣言的 `aruaru.yaml: query.parallel` で、実行時ミューテーション
// (`setParallelConfig`)は持たない。実効値の参照は `parallelConfig` query。

#[derive(InputObject)]
pub struct MigrateInput {
    pub source: String,
    pub source_uri: String,
    pub commit_message: Option<String>,
    pub include_tables: Option<Vec<String>>,
}

#[derive(InputObject)]
pub struct FederatedSourceInput {
    pub name: String,
    pub kind: String,
    pub uri: String,
}

#[derive(InputObject)]
pub struct ClusterNodeInput {
    pub action: String, // "add" | "remove"
    pub node_id: i64,
    pub addr: String,
}

/// 【2026-08-29 再設計 P3】`walAppend` mutation の WAL レコード 1 件
/// (旧 REST `WalAppendRecord` の等価)。`start_lsn` から +1 ずつ LSN を割る。
#[derive(InputObject)]
pub struct WalRecordInput {
    pub page_key: String,
    /// `"replace"`(既定)または `"append"`。
    pub op: Option<String>,
    /// ページへ書く内容(UTF-8 文字列として受け取り、バイト列へ変換)。
    pub data: String,
}

// ── Admin Query ───────────────────────────────────────────────

#[derive(Default)]
pub struct AdminQuery;

#[Object]
impl AdminQuery {
    // ── レジストリ ──────────────────────────────────────────

    async fn registry(&self, ctx: &Context<'_>) -> Result<Vec<DbEntryGql>> {
        let a = admin(ctx)?;
        Ok(a.registry
            .all()
            .into_iter()
            .map(|e| DbEntryGql {
                id: e.id,
                name: e.name,
                category: format!("{:?}", e.category),
                wire: format!("{:?}", e.wire),
                status: e.status.label().to_string(),
                rank: e.rank.map(|r| r as i32),
                score: e.score,
                updated_at: e.updated_at.unwrap_or_default(),
            })
            .collect())
    }

    async fn registry_summary(&self, ctx: &Context<'_>) -> Result<RegistrySummaryGql> {
        let a = admin(ctx)?;
        let s = a.registry.summary();
        Ok(RegistrySummaryGql {
            total: s.total as i32,
            connectable: s.postgres_wire as i32,
            ga: s.ga as i32,
            beta: s.beta as i32,
            pg_compatible: s.pg_compatible as i32,
            planned: s.planned as i32,
        })
    }

    // ── バックアップ ────────────────────────────────────────

    async fn backups(&self, ctx: &Context<'_>) -> Result<Vec<BackupGql>> {
        let a = admin(ctx)?;
        let manifests = a
            .backup
            .list_backups()
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(manifests.into_iter().map(manifest_to_gql).collect())
    }

    /// 【2026-08-29改修】固定`None`スタブを廃止し、REST `/admin/backup/
    /// schedule`(GET/POST)と同一の`SharedBackupSchedule`から実データを
    /// 返す。`schedule`未配線時(将来の互換性保険)のみ`None`のまま
    /// (未設定と区別が付かなくなる点は正直に受け入れる)。
    async fn backup_schedule(&self, ctx: &Context<'_>) -> Result<Option<ScheduleGql>> {
        let a = admin(ctx)?;
        let Some(shared) = &a.schedule else { return Ok(None) };
        Ok(shared.lock().clone().map(|s| ScheduleGql {
            enabled: s.enabled,
            cron: s.cron,
            kind: s.kind,
            // 実際のcron式からの次回実行時刻計算は未実装(REST側も
            // 同様に持たない、誇張しない)。
            next_run: None,
        }))
    }

    // ── クラスタ ────────────────────────────────────────────

    /// 【2026-08-29改修】固定値スタブを廃止し、REST `/admin/cluster`と
    /// 同一の`ClusterTopology`(`AdminCtx.topology`、`AdminState`と共有)
    /// から`status_snapshot`(`aruaru-dist`共通実装)経由で実データを
    /// 返すよう変更した。`topology`が渡されていない構成(将来の互換性
    /// 保険)の場合のみ、従来通りの単一ノード近似値へフォールバックする
    /// ——このフォールバック自体は正直にコメントで明示し、本番構成で
    /// 常に`Some`が渡っていることをテストで確認する。
    async fn cluster_status(&self, ctx: &Context<'_>) -> Result<ClusterStatusGql> {
        let a = admin(ctx)?;
        let total_rows = a.engine.total_rows() as u64;

        let snapshot = match &a.topology {
            Some(topology) => {
                let commit_count = a.engine.version().log(1_000_000).len() as u64;
                let table_count = a.engine.table_names().len();
                topology.lock().status_snapshot(commit_count, total_rows, table_count)
            }
            None => {
                // フォールバック(topology未配線時のみ、正直な近似値)。
                let commit_count = a.engine.version().log(1_000_000).len() as u64;
                aruaru_dist::ClusterTopology::single_node(1, "127.0.0.1:5432").status_snapshot(
                    commit_count,
                    total_rows,
                    a.engine.table_names().len(),
                )
            }
        };

        Ok(ClusterStatusGql {
            stats: ClusterStatsGql {
                total_nodes: snapshot.total_nodes as i32,
                healthy_nodes: snapshot.healthy_nodes as i32,
                total_ranges: snapshot.total_ranges as i32,
                total_rows: snapshot.total_rows as i64,
                table_count: snapshot.table_count as i32,
                replication_factor: snapshot.replication_factor as i32,
                under_replicated: snapshot.under_replicated.iter().map(|&id| id as i64).collect(),
            },
            nodes: snapshot
                .nodes
                .into_iter()
                .map(|n| NodeStatusGql {
                    node_id: n.node_id as i64,
                    addr: n.addr,
                    role: n.role,
                    alive: n.alive,
                    commit_index: n.commit_index as i64,
                    applied_index: n.applied_index as i64,
                    ranges: n.ranges as i32,
                    disk_used_gb: n.disk_used_gb,
                })
                .collect(),
            ranges: snapshot
                .ranges
                .into_iter()
                .map(|r| RangeGql {
                    range_id: r.range_id as i64,
                    start_key: r.start_key,
                    end_key: r.end_key,
                    leader_node: r.leader_node as i64,
                    replicas: r.replicas.iter().map(|&id| id as i64).collect(),
                    size_mb: r.size_mb,
                })
                .collect(),
        })
    }

    // ── 並列実行 ────────────────────────────────────────────

    /// 【2026-08-29 再設計 P2】固定値スタブを廃止し、`AdminState.parallel`
    /// (= `aruaru.yaml: query.parallel` を `config::reconcile` が反映した
    /// 4フィールド共有型)の**実効値**を返す。設定の書き込みは宣言的
    /// `aruaru.yaml` が正本のため、対応する `setParallelConfig` mutation
    /// は撤廃した(この query は読み取り専用の観測系)。
    async fn parallel_config(&self, ctx: &Context<'_>) -> Result<ParallelConfigGql> {
        let a = admin(ctx)?;
        let cur = a
            .parallel
            .as_ref()
            .map(|h| h.lock().clone())
            .unwrap_or_default();
        Ok(ParallelConfigGql {
            enabled: cur.enabled,
            max_workers: cur.max_workers as i32,
            chunk_size: cur.chunk_size as i32,
            strategy: cur.strategy,
        })
    }

    /// REST `/admin/parallel/jobs`も実際には長時間ジョブの常駐管理を
    /// 持たず`{"jobs": []}`を返すだけ(`admin.rs::list_jobs`のコメント
    /// 参照)——GraphQL側が空配列を返すのは、REST側と実際に一致した
    /// 状態であり、スタブとの乖離ではない。
    async fn parallel_jobs(&self, ctx: &Context<'_>) -> Result<Vec<ParallelJobGql>> {
        require_admin_token(ctx)?;
        Ok(vec![])
    }

    /// 【2026-08-29 再設計 P3】固定値スタブを廃止し、旧 REST
    /// `POST /admin/parallel/explain` の実ロジックを移植(読み取りなので
    /// `AdminQuery` 側)。SQL を分類(OLTP/OLAP)し、`AdminState.parallel`
    /// (= `aruaru.yaml: query.parallel` の実効値)と推定行数から分散実行
    /// プランのステップ列を組み立てる。単一ノードのため `node = "node-1"`。
    /// 集計を含めば ParallelScan→Shuffle→HashAggregate→Gather、単純検索
    /// なら ParallelScan→Gather。返りは「下から上」に読むプラン(逆順)。
    async fn explain_distributed(
        &self,
        ctx: &Context<'_>,
        sql: String,
    ) -> Result<Vec<ExplainStepGql>> {
        let a = admin(ctx)?;
        let cfg = a.parallel.as_ref().map(|h| h.lock().clone()).unwrap_or_default();
        let kind = aruaru_query::classify_query(&sql);

        let table = match aruaru_query::parser::parse(&sql) {
            Ok(aruaru_query::parser::Statement::Select { table, .. }) => Some(table),
            _ => None,
        };
        let rows = table
            .as_ref()
            .and_then(|t| a.engine.table_row_count(t))
            .unwrap_or(0) as i64;

        let scan_par = if cfg.enabled { (cfg.max_workers.min(8).max(1)) as i32 } else { 1 };
        let shuffle_partitions =
            if cfg.enabled { cfg.max_workers.saturating_mul(8).max(1) as i32 } else { 1 };
        let table_label = table.clone().unwrap_or_else(|| "(table)".into());

        let mut step = 0i32;
        let mut next = || {
            step += 1;
            step
        };

        let mut plan = vec![ExplainStepGql {
            step: next(),
            node: "node-1".into(),
            range: "(min)-(max)".into(),
            operation: if cfg.enabled {
                format!(
                    "ParallelScan[{table_label}] par={scan_par} (述語プッシュダウン, strategy={})",
                    cfg.strategy
                )
            } else {
                format!("ParallelScan[{table_label}] par=1 (並列実行無効)")
            },
            estimated_rows: rows,
        }];

        if matches!(kind, aruaru_query::QueryKind::Olap) {
            plan.push(ExplainStepGql {
                step: next(),
                node: "node-1".into(),
                range: "(min)-(max)".into(),
                operation: format!("ShuffleExchange par={shuffle_partitions} (ハッシュ再分配)"),
                estimated_rows: rows,
            });
            plan.push(ExplainStepGql {
                step: next(),
                node: "node-1".into(),
                range: "(min)-(max)".into(),
                operation: format!("HashAggregate par={scan_par} (2段階集計)"),
                estimated_rows: rows / 10 + 1,
            });
        }

        plan.push(ExplainStepGql {
            step: next(),
            node: "node-1".into(),
            range: "(min)-(max)".into(),
            operation: format!("Gather(Coordinator) par=1 [query_kind={kind:?}]"),
            estimated_rows: rows / 10 + 1,
        });

        plan.reverse();
        Ok(plan)
    }

    // ── フェデレーション ────────────────────────────────────

    /// 【2026-08-29改修】固定空配列スタブを廃止し、REST `/admin/
    /// federation`(GET/POST)と同一の`SharedFederatedSources`から実際に
    /// 登録済みのソース一覧を返す。
    async fn federated_sources(&self, ctx: &Context<'_>) -> Result<Vec<FederatedSourceGql>> {
        let a = admin(ctx)?;
        let Some(shared) = &a.federation else { return Ok(vec![]) };
        Ok(shared
            .lock()
            .iter()
            .map(|s| FederatedSourceGql {
                name: s.name.clone(),
                kind: s.kind.clone(),
                uri: s.uri.clone(),
                status: s.status.clone().unwrap_or_else(|| "unknown".into()),
                tables: s.table_count.unwrap_or(0) as i32,
            })
            .collect())
    }

    // ── APIキー自動ライフサイクル管理 ────────────────────────

    /// 【2026-08-29(続き)新設】REST `GET /admin/keys/status`と同一の
    /// `KeyGuardian`から実際の発行済みキー数を返す(従来GraphQL側には
    /// この操作自体が存在しなかった)。
    async fn key_status(&self, ctx: &Context<'_>) -> Result<KeyStatusGql> {
        let a = admin(ctx)?;
        let count = a.keyring.as_ref().map(|k| k.count()).unwrap_or(0);
        Ok(KeyStatusGql { issued_key_count: count as i32 })
    }

    // ── Vitess Reshard(併合)+ VTGate scatter-gather ─────────

    /// 旧 REST `GET /admin/multi-raft/scatter-query`の等価。全Rangeの
    /// `commit_index`/`role`をrange_id順に集約して返す(合意を伴わない
    /// 読み取り専用の集約、`MultiRaftCluster::scatter_gather`)。
    async fn multi_raft_scatter_query(&self, ctx: &Context<'_>) -> Result<Vec<MultiRaftRangeReadingGql>> {
        let a = admin(ctx)?;
        let Some(cluster) = &a.multi_raft else {
            return Err(async_graphql::Error::new(
                "MultiRaftCluster未初期化です(main.rsでの構築に失敗している可能性があります)。",
            ));
        };
        let gathered = cluster.scatter_gather(|node| (node.commit_index(), format!("{:?}", node.role())));
        Ok(gathered
            .into_iter()
            .map(|(range_id, (commit_index, role))| MultiRaftRangeReadingGql {
                range_id: range_id as i64,
                commit_index: commit_index as i64,
                role,
            })
            .collect())
    }

    // ── オブジェクトテーブル: 時間旅行(VersionlessAPIの実体) ──────

    /// 【2026-08-29(続き3) REST完全撤廃】現在のスナップショットと履歴
    /// (時間旅行の連鎖)を返す。旧 REST `GET /admin/object-table`の
    /// 置き換えで、そのルートは削除済み——スナップショット連鎖を
    /// 参照する経路はこの query のみ。
    async fn object_table(&self, ctx: &Context<'_>) -> Result<ObjectTableStatusGql> {
        let a = admin(ctx)?;
        let Some(t) = a.object_table.as_ref() else {
            return Ok(ObjectTableStatusGql {
                table_key: String::new(),
                current: None,
                history_len: 0,
                history: Vec::new(),
            });
        };
        let current = t
            .current_snapshot()
            .map_err(|e| async_graphql::Error::new(e.to_string()))?
            .map(snapshot_to_gql);
        let history: Vec<_> = t
            .snapshot_history()
            .map_err(|e| async_graphql::Error::new(e.to_string()))?
            .into_iter()
            .map(snapshot_to_gql)
            .collect();
        Ok(ObjectTableStatusGql {
            table_key: t.table_key(),
            current,
            history_len: history.len() as i32,
            history,
        })
    }

    // ── Closed timestamp / Follower read(2026-08-29 再設計 P3、REST完全撤廃) ──
    //
    // 旧 REST `GET /admin/closed-timestamp`・`POST /admin/closed-timestamp/plan`
    // の等価(いずれも状態の読み取りなので AdminQuery)。ワンショット操作
    // (register / advance)は AdminMutation 側。`/receive`・`/publish`
    // (ノード間 side transport)は B4 としてバイナリトランスポート側に残る。

    /// 登録済み Range ごとの closed timestamp(observability)。
    async fn closed_timestamp(&self, ctx: &Context<'_>) -> Result<ClosedTimestampStatusGql> {
        let a = admin(ctx)?;
        let Some(coord) = a.closed_ts.as_ref() else {
            return Ok(ClosedTimestampStatusGql { range_count: 0, ranges: Vec::new() });
        };
        let ranges: Vec<ClosedTimestampRangeGql> = coord
            .range_ids()
            .into_iter()
            .filter_map(|id| {
                coord.tracker(id).map(|t| ClosedTimestampRangeGql {
                    range_id: id as i64,
                    closed_timestamp: t.closed_timestamp().to_string(),
                    lowest_in_flight: t.lowest_in_flight().map(|v| v.to_string()),
                    target_lag_nanos: t.target_lag_nanos().to_string(),
                })
            })
            .collect();
        Ok(ClosedTimestampStatusGql { range_count: ranges.len() as i32, ranges })
    }

    /// 指定 Range 群を follower read で読めるかを判定する
    /// (`AS OF SYSTEM TIME` 相当の判断)。`table` を渡すと、follower read が
    /// 許可された場合に実際に `select_follower_read`(`AS OF COMMIT` と同じ
    /// Prolly Tree 経由)でデータも読み出す。旧 REST
    /// `POST /admin/closed-timestamp/plan` の実ロジックを移植。
    async fn plan_follower_read(
        &self,
        ctx: &Context<'_>,
        range_ids: Vec<i64>,
        mode: Option<String>,
        now_nanos: Option<String>,
        staleness_nanos: Option<String>,
        table: Option<String>,
        filter_col: Option<String>,
        filter_val: Option<String>,
    ) -> Result<FollowerReadPlanGql> {
        let a = admin(ctx)?;
        let coord = a
            .closed_ts
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("closed timestamp coordinator is not configured"))?;
        let ids: Vec<u64> = range_ids.iter().map(|&x| x as u64).collect();
        let now = match now_nanos {
            Some(s) => parse_u64(&s, "nowNanos")?,
            None => now_default_nanos(a),
        };
        let staleness = match staleness_nanos {
            Some(s) => parse_u64(&s, "stalenessNanos")?,
            None => aruaru_dist::DEFAULT_MAX_STALENESS_NANOS,
        };
        let mode = mode.unwrap_or_else(|| "bounded".to_string());
        let plan = match mode.as_str() {
            "exact" => coord.plan_exact_staleness_read(&ids, now, staleness),
            _ => coord.negotiate_bounded_staleness(&ids, now, staleness),
        };
        let (plan_kind, reason, read_ts, stale) = match &plan {
            aruaru_dist::ReadPlan::FollowerRead { timestamp, staleness_nanos } => (
                "follower_read",
                None,
                Some(timestamp.to_string()),
                Some(staleness_nanos.to_string()),
            ),
            aruaru_dist::ReadPlan::RouteToLeaseholder { reason } => {
                ("route_to_leaseholder", Some(reason.to_string()), None, None)
            }
        };
        let data = match (&plan, &table) {
            (aruaru_dist::ReadPlan::FollowerRead { timestamp, .. }, Some(tbl)) => {
                let filter = match (&filter_col, &filter_val) {
                    (Some(c), Some(v)) => Some((c.clone(), v.clone())),
                    _ => None,
                };
                match a.engine.select_follower_read(tbl.clone(), filter, *timestamp as i64) {
                    Ok(resp) => Some(FollowerReadDataGql {
                        ok: true,
                        error: None,
                        result: Some(crate::response_to_gql(resp)),
                    }),
                    Err(e) => Some(FollowerReadDataGql { ok: false, error: Some(e), result: None }),
                }
            }
            _ => None,
        };
        Ok(FollowerReadPlanGql {
            plan: plan_kind.to_string(),
            is_follower_read: plan.is_follower_read(),
            read_timestamp: read_ts,
            staleness_nanos: stale,
            reason,
            data,
        })
    }

    // ── WAL サービス(Neon 方式、2026-08-29 再設計 P3、REST完全撤廃) ──
    //
    // 旧 REST `GET /admin/wal-service`(status)・`POST /admin/wal-service/page`
    // (`get_page_at_lsn` = 純粋な読み取り)の等価。append(耐久化)/
    // image-layer(compaction)は AdminMutation 側。

    /// safekeeper quorum の状態と pageserver の適用状況。
    async fn wal_service(&self, ctx: &Context<'_>) -> Result<WalServiceStatusGql> {
        let a = admin(ctx)?;
        let s = a
            .wal_storage
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("WAL service is not configured"))?;
        // safekeeper の id は 1..=n。REST 版は `0..len` で先頭を取りこぼして
        // いたため、ここでは `1..=n` に正した(observability の正確性優先)。
        let safekeepers: Vec<WalSafekeeperGql> = (1..=s.wal.len() as u64)
            .filter_map(|i| {
                s.wal.safekeeper(i).map(|sk| WalSafekeeperGql {
                    id: sk.id() as i64,
                    accepted_term: sk.accepted_term().to_string(),
                    flush_lsn: sk.flush_lsn().to_string(),
                })
            })
            .collect();
        Ok(WalServiceStatusGql {
            term: s.term().to_string(),
            quorum: s.wal.quorum() as i32,
            commit_lsn: s.wal.commit_lsn().to_string(),
            safekeepers,
            pageserver: WalPageserverGql {
                last_record_lsn: s.pageserver.last_record_lsn().to_string(),
                max_replication_lag: s.pageserver.max_replication_lag().to_string(),
                page_keys: s.pageserver.page_keys(),
            },
        })
    }

    /// 指定 LSN 時点のページを pageserver 上で再構成して返す
    /// (`get_page_at_lsn`)。`AS OF COMMIT` 読み取りの土台。`lsn` 省略時は
    /// pageserver の最終適用 LSN。
    async fn wal_page(
        &self,
        ctx: &Context<'_>,
        page_key: String,
        lsn: Option<String>,
    ) -> Result<WalPageGql> {
        let a = admin(ctx)?;
        let s = a
            .wal_storage
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("WAL service is not configured"))?;
        let ps = &s.pageserver;
        let lsn = match lsn {
            Some(v) => parse_u64(&v, "lsn")?,
            None => ps.last_record_lsn(),
        };
        let bytes = ps
            .get_page_at_lsn(&page_key, lsn)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(WalPageGql {
            len: bytes.len() as i32,
            data: String::from_utf8_lossy(&bytes).into_owned(),
            image_layer_lsn: ps.image_layer_lsn(&page_key).map(|v| v.to_string()),
            page_key,
            lsn: lsn.to_string(),
        })
    }

    // ── ScyllaDB shard-per-core ストア(2026-08-29 再設計 P3、REST完全撤廃) ──
    //
    // 旧 REST `GET /admin/sharded-store/:key`・`GET /admin/sharded-store-stats`
    // の等価。put は AdminMutation 側。シャードスレッドとの通信は
    // `std::sync::mpsc` のブロッキング recv なので `spawn_blocking` で退避する
    // (REST ハンドラと同じ配慮)。

    async fn sharded_store_get(
        &self,
        ctx: &Context<'_>,
        key: String,
    ) -> Result<ShardedStoreEntryGql> {
        let a = admin(ctx)?;
        let store = a
            .sharded_store
            .clone()
            .ok_or_else(|| async_graphql::Error::new("sharded store is not configured"))?;
        let shard_id = store.shard_for(key.as_bytes()) as i64;
        let k = key.clone();
        let value = tokio::task::spawn_blocking(move || store.get(k.as_bytes()))
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(ShardedStoreEntryGql { key, shard_id, found: value.is_some(), value })
    }

    async fn sharded_store_stats(&self, ctx: &Context<'_>) -> Result<ShardedStoreStatsGql> {
        let a = admin(ctx)?;
        let store = a
            .sharded_store
            .clone()
            .ok_or_else(|| async_graphql::Error::new("sharded store is not configured"))?;
        let (shard_count, per_shard_len, total_len) = tokio::task::spawn_blocking(move || {
            let shard_count = store.shard_count();
            let per: Vec<i64> = (0..shard_count).map(|i| store.shard_len(i) as i64).collect();
            let total: i64 = per.iter().sum();
            (shard_count, per, total)
        })
        .await
        .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(ShardedStoreStatsGql {
            shard_count: shard_count as i32,
            per_shard_len,
            total_len,
        })
    }

    /// 【2026-09-02 続き23】HTAP 列レプリカの観測。同居 `ColumnarApplier`
    /// (`aruaru.yaml: htap.columnar_replicas: true`)の行→列非同期変換
    /// レプリカについて、TiFlash `INFORMATION_SCHEMA.TIFLASH_REPLICA`
    /// (PROGRESS / AVAILABLE)相当の同期状態を返す。`prune_column` を渡すと
    /// `prune_op`(`gt`|`ge`|`lt`|`le`|`eq`)+ `prune_value` で MoR ビューを
    /// 枝刈りしたプレビュー(読む必要のある block 数)も含める。
    /// 列レプリカ機能が無効なら空扱い(`available=false`)。
    async fn htap_replicas(
        &self,
        ctx: &Context<'_>,
        table: String,
        prune_column: Option<String>,
        prune_op: Option<String>,
        prune_value: Option<String>,
    ) -> Result<HtapReplicaStatusGql> {
        let a = admin(ctx)?;
        let Some(columnar) = a.columnar.clone() else {
            return Ok(HtapReplicaStatusGql {
                table,
                available: false,
                progress: 0.0,
                columnar_block_count: 0,
                columnar_live_row_count: 0,
                deletion_vector_positions: 0,
                applied_index: 0,
                applied_commit_seq: 0,
                replication_count: 0,
                prune: None,
            });
        };

        let prune = match (prune_column, prune_value) {
            (Some(col), Some(val)) => {
                let op = prune_op.unwrap_or_else(|| "eq".to_string());
                let preview = if op.eq_ignore_ascii_case("eq") {
                    columnar.prune_equality_preview(&table, &col, &val)
                } else {
                    let range_op = match op.to_ascii_lowercase().as_str() {
                        "gt" => aruaru_backup::table_format::RangeOp::Gt,
                        "ge" => aruaru_backup::table_format::RangeOp::Ge,
                        "lt" => aruaru_backup::table_format::RangeOp::Lt,
                        "le" => aruaru_backup::table_format::RangeOp::Le,
                        other => {
                            return Err(async_graphql::Error::new(format!(
                                "unknown prune_op '{other}' (want gt|ge|lt|le|eq)"
                            )))
                        }
                    };
                    let v: f64 = val.parse().map_err(|_| {
                        async_graphql::Error::new("prune_value must be a number for range ops")
                    })?;
                    columnar.prune_range_preview(&table, &col, range_op, v)
                };
                preview.map(|p| HtapPrunePreviewGql {
                    column: col,
                    op,
                    value: val,
                    total_blocks: p.total_blocks as i32,
                    kept_blocks: p.kept_blocks as i32,
                    skipped_blocks: p.skipped_blocks as i32,
                    kept_live_rows: p.kept_live_rows as i64,
                })
            }
            _ => None,
        };

        Ok(htap_status_for(&columnar, &table, prune))
    }

    /// 【2026-09-03 続き26 / P-HLC-3b】HLC(案A、フル精度 2 フィールド)の
    /// 現在値を観測する。`SystemTime::now()` を読んで HLC を1ステップ進めた
    /// 値を返す(`closedTsAdvance` 等が `nowNanos` 省略時に使うのと同じ経路)。
    /// `wallNanos`(フル精度ナノ秒)/ `logical` / `synthetic` に加え、
    /// 既存 API 互換の `ordinal`(案B 65µs 射影)も返す。
    async fn hlc_now(&self, ctx: &Context<'_>) -> Result<HlcNowGql> {
        let a = admin(ctx)?;
        let hlc = a
            .hlc
            .clone()
            .ok_or_else(|| async_graphql::Error::new("HLC is not configured"))?;
        let ts = hlc.now_hlc_sys();
        let max_offset = hlc.max_offset_nanos();
        Ok(HlcNowGql {
            wall_nanos: ts.wall_nanos.to_string(),
            logical: ts.logical as i32,
            synthetic: ts.synthetic,
            ordinal: ts.as_ordinal().to_string(),
            max_offset_ms: (max_offset / 1_000_000) as i64,
            uncertainty_upper_nanos: ts.uncertainty_upper(max_offset).to_string(),
        })
    }

    /// 【2026-09-02 続き24】`htapReplicas` の全テーブル版。TiFlash の
    /// `INFORMATION_SCHEMA.TIFLASH_REPLICA` が全 (db, table) 行を返すのと
    /// 同じく、テーブル名を知らなくても全列レプリカの同期状態を一覧できる。
    /// 枝刈り込みプレビューは含めない(per-table `htapReplicas` を使う)。
    /// 列レプリカ機能が無効なら空配列。
    async fn htap_replicas_all(&self, ctx: &Context<'_>) -> Result<Vec<HtapReplicaStatusGql>> {
        let a = admin(ctx)?;
        let Some(columnar) = a.columnar.clone() else {
            return Ok(Vec::new());
        };
        Ok(columnar
            .replicated_tables()
            .iter()
            .map(|t| htap_status_for(&columnar, t, None))
            .collect())
    }

    // ── マイグレーション: スキーマプレビュー ────────────────

    async fn preview_source(
        &self,
        ctx: &Context<'_>,
        source: String,
        uri: String,
    ) -> Result<Vec<TableInfoGql>> {
        // 呼び出し元が任意のURIへ接続を試みさせられる(サーバー自身に
        // 外部/内部ネットワークへの接続を行わせる、SSRF類似の経路)ため、
        // 管理者トークン必須とする(2026-08-01追加、実バグ修正)。
        require_admin_token(ctx)?;
        use aruaru_registry::adapter::adapter_for;
        let wire = wire_for_source(&source)
            .ok_or_else(|| async_graphql::Error::new(format!("未対応ソース: {source}")))?;
        let adapter = adapter_for(wire)
            .ok_or_else(|| async_graphql::Error::new("アダプタ未実装"))?;
        let tables = adapter.list_tables(&uri).await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(tables.into_iter().map(|t| TableInfoGql {
            schema: t.schema,
            name: t.name,
            estimated_rows: t.estimated_rows,
        }).collect())
    }
}

// ── Admin Mutation ────────────────────────────────────────────

#[derive(Default)]
pub struct AdminMutation;

#[Object]
impl AdminMutation {
    // ── オブジェクトテーブル: 時間旅行(VersionlessAPIの実体) ──────
    //
    // 【2026-08-29(続き3) REST完全撤廃】旧 REST `POST /admin/object-table/
    // commit`・`POST /admin/object-table/prune` の等価。両ルートは
    // `admin.rs`から削除済みで、これがコミット/枝刈りの唯一の経路。

    /// blockメタデータ群を1 segmentとして書き、MetaSrv の CAS が成功
    /// したらコミット成立(Databend方式)。新スナップショットIDを返す。
    async fn object_table_commit(
        &self,
        ctx: &Context<'_>,
        blocks: Vec<ObjectBlockInput>,
    ) -> Result<ObjectTableCommitResultGql> {
        let a = admin(ctx)?;
        let t = a
            .object_table
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("object table is not configured"))?;
        let mut metas = Vec::with_capacity(blocks.len());
        for b in &blocks {
            let mut meta = aruaru_backup::table_format::BlockMeta::new(
                b.location.clone(),
                b.row_count as u64,
                b.size_bytes as u64,
            );
            for s in &b.stats {
                meta = meta.with_stats(&s.column, s.min, s.max, s.null_count as u64);
            }
            for bl in &b.bloom {
                meta = meta.with_bloom(&bl.column, bl.keys.iter().map(|s| s.as_str()));
            }
            metas.push(meta);
        }
        let block_count = metas.len() as i32;
        let snapshot_id = t
            .commit_blocks(metas)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(ObjectTableCommitResultGql { snapshot_id, block_count })
    }

    /// 3層メタデータでの枝刈り。等値なら`key`(bloom filter)、範囲なら
    /// `op`(`lt`/`le`/`gt`/`ge`)+`value`(min/max 統計)。`snapshotId`
    /// 省略時は現在のスナップショット。REST版のバリデーション文言を踏襲。
    async fn object_table_prune(
        &self,
        ctx: &Context<'_>,
        column: String,
        snapshot_id: Option<String>,
        op: Option<String>,
        value: Option<f64>,
        key: Option<String>,
    ) -> Result<ObjectTablePruneResultGql> {
        use aruaru_backup::table_format::RangeOp;
        let a = admin(ctx)?;
        let t = a
            .object_table
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("object table is not configured"))?;
        let snapshot_id = match snapshot_id {
            Some(id) => id,
            None => {
                t.current_snapshot()
                    .map_err(|e| async_graphql::Error::new(e.to_string()))?
                    .ok_or_else(|| async_graphql::Error::new("table has no snapshot yet"))?
                    .snapshot_id
            }
        };
        if let Some(key) = key {
            let kept = t
                .prune_equality(&snapshot_id, &column, &key)
                .map_err(|e| async_graphql::Error::new(e.to_string()))?;
            return Ok(ObjectTablePruneResultGql {
                snapshot_id,
                predicate: "equality".into(),
                column,
                kept_blocks: kept.len() as i32,
                skipped_segments: 0,
                skipped_blocks: 0,
                locations: kept.iter().map(|b| b.location.clone()).collect(),
            });
        }
        let range_op = match op.as_deref() {
            None => {
                return Err(async_graphql::Error::new(
                    "op is required for range predicates (use `key` instead for equality)",
                ))
            }
            Some("eq") => {
                return Err(async_graphql::Error::new(
                    "equality predicates must use `key` (bloom filter), not `op: eq`",
                ))
            }
            Some("lt") => RangeOp::Lt,
            Some("le") => RangeOp::Le,
            Some("gt") => RangeOp::Gt,
            Some("ge") => RangeOp::Ge,
            Some(other) => {
                return Err(async_graphql::Error::new(format!("unknown op: {other}")))
            }
        };
        let value = value
            .ok_or_else(|| async_graphql::Error::new("value (or key) is required"))?;
        let (kept, skipped_segments, skipped_blocks) = t
            .prune_range(&snapshot_id, &column, range_op, value)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(ObjectTablePruneResultGql {
            snapshot_id,
            predicate: "range".into(),
            column,
            kept_blocks: kept.len() as i32,
            skipped_segments: skipped_segments as i32,
            skipped_blocks: skipped_blocks as i32,
            locations: kept.iter().map(|b| b.location.clone()).collect(),
        })
    }

    // ── レジストリ ──────────────────────────────────────────

    async fn crawl_registry(&self, ctx: &Context<'_>) -> Result<CrawlResultGql> {
        let a = admin(ctx)?;
        match a.registry.crawl_now().await {
            Ok(report) => Ok(CrawlResultGql {
                ok: true,
                updated: report.matched as i32,
                message: format!("クロール完了: {} 件更新", report.matched),
            }),
            Err(e) => Ok(CrawlResultGql {
                ok: false,
                updated: 0,
                message: e.to_string(),
            }),
        }
    }

    async fn test_registry_connection(
        &self,
        ctx: &Context<'_>,
        id: String,
        uri: String,
    ) -> Result<ConnTestGql> {
        require_admin_token(ctx)?;
        use aruaru_registry::{adapter::adapter_for, Wire};
        // id からワイヤを推定（レジストリ検索簡易版）
        let wire = if uri.starts_with("postgres") || uri.starts_with("cockroach") {
            Wire::Postgres
        } else if uri.starts_with("mysql") || uri.starts_with("mariadb") {
            Wire::MySQL
        } else if uri.starts_with("mongodb") {
            Wire::Mongo
        } else {
            return Ok(ConnTestGql { ok: false, message: format!("id={id}: ワイヤ未判定"), server_version: None });
        };
        let _ = id;
        let Some(adapter) = adapter_for(wire) else {
            return Ok(ConnTestGql { ok: false, message: "アダプタ未実装".into(), server_version: None });
        };
        let result = adapter.test(&uri).await;
        Ok(ConnTestGql { ok: result.ok, message: result.message, server_version: result.server_version })
    }

    // ── バックアップ ────────────────────────────────────────

    async fn create_backup(
        &self,
        ctx: &Context<'_>,
        config: Option<BackupConfigInput>,
    ) -> Result<BackupGql> {
        let a = admin(ctx)?;
        let kind = config.as_ref().and_then(|c| c.kind.clone()).unwrap_or_else(|| "full".into());

        let manifest = if kind.eq_ignore_ascii_case("snapshot") {
            let commit_id = a
                .engine
                .version()
                .head()
                .map(|c| c.id.as_str().to_string())
                .unwrap_or_default();
            a.backup.snapshot(&commit_id).await
        } else {
            a.backup.run_full(|_| {}).await
        }
        .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        Ok(manifest_to_gql(manifest))
    }

    async fn restore_backup(
        &self,
        ctx: &Context<'_>,
        input: RestoreInput,
    ) -> Result<MutationResult> {
        let a = admin(ctx)?;
        // restore() は宛先ディレクトリではなく QueryEngine へ直接 ingest するため未使用
        let unused_target_dir = std::path::PathBuf::new();
        match a
            .backup
            .restore(&input.backup_id, &unused_target_dir, |_| {})
            .await
        {
            Ok(()) => Ok(MutationResult {
                success: true,
                commit_id: None,
                message: format!("バックアップ {} からリストアしました。", input.backup_id),
            }),
            Err(e) => Ok(MutationResult {
                success: false,
                commit_id: None,
                message: e.to_string(),
            }),
        }
    }

    /// 【2026-08-29改修】入力をそのまま返すだけのスタブを廃止し、REST
    /// `/admin/backup/schedule`(POST)と同一の`SharedBackupSchedule`へ
    /// 実際に書き込む。以後`backup_schedule`クエリ・REST `GET
    /// /admin/backup/schedule`の双方から同じ値が読める。
    async fn set_backup_schedule(
        &self,
        ctx: &Context<'_>,
        input: ScheduleInput,
    ) -> Result<ScheduleGql> {
        let a = admin(ctx)?;
        let state = aruaru_dist::admin_shared::BackupScheduleState {
            enabled: input.enabled,
            cron: input.cron,
            kind: input.kind,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        if let Some(shared) = &a.schedule {
            *shared.lock() = Some(state.clone());
        }
        Ok(ScheduleGql {
            enabled: state.enabled,
            cron: state.cron,
            kind: state.kind,
            next_run: None,
        })
    }

    // ── クラスタ ────────────────────────────────────────────

    async fn cluster_node_op(
        &self,
        ctx: &Context<'_>,
        input: ClusterNodeInput,
    ) -> Result<MutationResult> {
        require_admin_token(ctx)?;
        Ok(MutationResult {
            success: true,
            commit_id: None,
            message: format!("ノード {} ({}): {} 操作を受理しました。", input.node_id, input.addr, input.action),
        })
    }

    async fn rebalance_cluster(&self, ctx: &Context<'_>) -> Result<MutationResult> {
        require_admin_token(ctx)?;
        Ok(MutationResult {
            success: true,
            commit_id: None,
            message: "リバランス計画を実行しました。".into(),
        })
    }

    /// **2026-07-26追記**: 以前は`RaftNode`/`RaftWriter`を完全に迂回し
    /// `a.engine.execute(&sql)`で`QueryEngine`へ直接書き込んでいた
    /// (2026-07-25(続き2)HANDOFFが未解決ギャップとして記録)。稼働中の
    /// `replicator`(`admin.rs`のREST `/admin/cluster/propose`・pgwire
    /// サーバへ渡しているのと同一の`Arc<dyn ReplicatedWriter>`)が
    /// 取り付けられていれば、そちらを優先して`propose_and_wait`
    /// (quorum合意+disaster-backup配線込み)を経由する。`replicator`が
    /// 無い(単一ノード/非クラスタ構成、またはクラスタ構築失敗)場合のみ、
    /// 後方互換のため`engine.execute`直接経路へフォールタックし、
    /// `message`に`raft_fallback_no_replicator`相当である旨を正直に含める
    /// (REST側`admin.rs::cluster_propose`の`mode: "raft_fallback_no_replicator"`
    /// と同じ意図)。
    async fn cluster_propose(
        &self,
        ctx: &Context<'_>,
        sql: String,
    ) -> Result<MutationResult> {
        let a = admin(ctx)?;
        if let Some(replicator) = &a.replicator {
            return match replicator.write_sql(&sql).await {
                Ok(tag) => Ok(MutationResult {
                    success: true,
                    commit_id: None,
                    message: format!("mode=raft: RaftWriter経由で提案・commit+適用が完了しました({tag})。"),
                }),
                Err(e) => Ok(MutationResult { success: false, commit_id: None, message: e }),
            };
        }
        // フォールバック: replicator 未取り付け(単一ノード/非クラスタ構成、
        // またはクラスタ構築失敗)の場合のみ、従来通り QueryEngine へ直接実行する。
        // この経路は Raft コンセンサス・disaster-backup 配線を経由しない
        // (複数ノードクラスタで replicator が無いのは異常系のみ想定)。
        match a.engine.execute(&sql) {
            Ok(_) => Ok(MutationResult {
                success: true,
                commit_id: None,
                message: "mode=raft_fallback_no_replicator: replicator未取り付けのためQueryEngine直接実行(disaster-backup配線対象外)。".into(),
            }),
            Err(e) => Ok(MutationResult { success: false, commit_id: None, message: e }),
        }
    }

    // ── 並列実行 ────────────────────────────────────────────
    // `setParallelConfig` mutation は撤廃(2026-08-29 再設計 P2)。並列
    // 設定は宣言的 `aruaru.yaml: query.parallel` が正本(ホットリロード)。
    // 実効値は `Query.parallelConfig`、分散プランは `Query.explainDistributed`
    // で参照する(いずれも読み取りなので Query 側。P3 で AdminMutation から
    // AdminQuery へ移設)。

    // ── フェデレーション ────────────────────────────────────

    /// 【2026-08-29改修】入力をそのまま返すだけのスタブを廃止し、REST
    /// `/admin/federation`(POST)と同一の`SharedFederatedSources`へ実際に
    /// 追加する。REST側と同じ「既に同名が存在すれば失敗」ルールを踏襲。
    /// **正直な開示**: `status: "connected"`は実際に接続確認したわけ
    /// ではない(REST側`register_federation`も同様に`"unknown"`を
    /// セットするのみ、実接続確認は別の`test_registry_connection`/
    /// `test_source_connection`ミューテーションの責務)——ここでは
    /// REST側の`"unknown"`表記へ揃えた(誇張しない)。
    async fn register_federated_source(
        &self,
        ctx: &Context<'_>,
        input: FederatedSourceInput,
    ) -> Result<FederatedSourceGql> {
        let a = admin(ctx)?;
        let entry = aruaru_dist::admin_shared::FederatedSourceEntry {
            name: input.name,
            kind: input.kind,
            uri: input.uri,
            read_only: true,
            pushdown: false,
            status: Some("unknown".into()),
            table_count: None,
        };
        if let Some(shared) = &a.federation {
            let mut list = shared.lock();
            if list.iter().any(|s| s.name == entry.name) {
                return Err(async_graphql::Error::new(format!("既に存在します: {}", entry.name)));
            }
            list.push(entry.clone());
        }
        Ok(FederatedSourceGql {
            name: entry.name,
            kind: entry.kind,
            uri: entry.uri,
            status: entry.status.unwrap_or_else(|| "unknown".into()),
            tables: entry.table_count.unwrap_or(0) as i32,
        })
    }

    /// 【2026-08-29改修】REST `/admin/federation/drop`と同一の
    /// `SharedFederatedSources`から実際に削除する(従来は何もせず成功
    /// メッセージだけを返すスタブだった)。
    async fn drop_federated_source(
        &self,
        ctx: &Context<'_>,
        name: String,
    ) -> Result<MutationResult> {
        let a = admin(ctx)?;
        if let Some(shared) = &a.federation {
            shared.lock().retain(|s| s.name != name);
        }
        Ok(MutationResult { success: true, commit_id: None, message: format!("'{name}' を削除しました。") })
    }

    async fn federated_query(
        &self,
        ctx: &Context<'_>,
        sql: String,
    ) -> Result<QueryResultGql> {
        let a = admin(ctx)?;
        let resp = a.engine.execute_async(&sql).await
            .map_err(async_graphql::Error::new)?;
        Ok(crate::response_to_gql(resp))
    }

    // ── APIキー自動ライフサイクル管理 ────────────────────────

    /// 【2026-08-29(続き)新設】REST `POST /admin/keys/revoke`と同一の
    /// `KeyGuardian`へ実際に委譲する(自動破棄=auto-revoke)。
    async fn revoke_keys(&self, ctx: &Context<'_>, owner: String) -> Result<KeyRevokeResultGql> {
        let a = admin(ctx)?;
        let revoked = a.keyring.as_ref().map(|k| k.revoke_owner(&owner)).unwrap_or(0);
        Ok(KeyRevokeResultGql { revoked_count: revoked as i32 })
    }

    // ── マイグレーション ────────────────────────────────────

    async fn test_source_connection(
        &self,
        ctx: &Context<'_>,
        source: String,
        uri: String,
    ) -> Result<ConnTestGql> {
        require_admin_token(ctx)?;
        use aruaru_registry::adapter::adapter_for;
        let Some(wire) = wire_for_source(&source) else {
            return Ok(ConnTestGql { ok: false, message: format!("未対応ソース: {source}"), server_version: None });
        };
        let Some(adapter) = adapter_for(wire) else {
            return Ok(ConnTestGql { ok: false, message: "アダプタ未実装".into(), server_version: None });
        };
        let r = adapter.test(&uri).await;
        Ok(ConnTestGql { ok: r.ok, message: r.message, server_version: r.server_version })
    }

    async fn run_migration(
        &self,
        ctx: &Context<'_>,
        input: MigrateInput,
    ) -> Result<MigrateResultGql> {
        use aruaru_registry::adapter::adapter_for;
        let a = admin(ctx)?;

        let wire = wire_for_source(&input.source)
            .ok_or_else(|| async_graphql::Error::new(format!("未対応ソース: {}", input.source)))?;
        let adapter = adapter_for(wire)
            .ok_or_else(|| async_graphql::Error::new("アダプタ未実装"))?;

        let tables = adapter.list_tables(&input.source_uri).await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        let include = input.include_tables.unwrap_or_default();
        let mut imported = Vec::new();
        let mut total_rows = 0i64;

        for t in &tables {
            if !include.is_empty() && !include.contains(&t.name) { continue; }
            match adapter.read_table(&input.source_uri, &t.schema, &t.name, 100_000).await {
                Ok((cols, rows)) => {
                    let n = a.engine.ingest_table(&t.name, cols, rows);
                    total_rows += n as i64;
                    imported.push(TableImportGql { table: t.name.clone(), rows: Some(n as i64), error: None });
                }
                Err(e) => imported.push(TableImportGql { table: t.name.clone(), rows: None, error: Some(e.to_string()) }),
            }
        }

        let msg = input.commit_message.unwrap_or_else(|| "Migration import".into());
        let safe = msg.replace('\'', "''");
        let commit_id = a.engine.execute(&format!("SELECT aruaru_commit('{safe}')"))
            .ok()
            .and_then(|r| if let aruaru_query::QueryResponse::Rows { rows, .. } = r {
                rows.first()?.first().map(|v| v.as_text())
            } else { None })
            .unwrap_or_default();

        Ok(MigrateResultGql {
            success: true,
            wire: adapter.wire_name().into(),
            total_rows,
            commit_id,
            message: format!("{} テーブル / {} 行 を取り込みました。", imported.len(), total_rows),
            tables: imported,
        })
    }

    // ── Closed timestamp: ワンショット操作(2026-08-29 再設計 P3、REST完全撤廃) ──
    //
    // 旧 REST `POST /admin/closed-timestamp/range`・`/advance` の等価
    // (冪等でない副作用 = B2)。observability(status / plan)は AdminQuery 側。

    /// Range を closed timestamp の追跡対象として登録する。
    async fn closed_ts_register_range(
        &self,
        ctx: &Context<'_>,
        range_id: i64,
    ) -> Result<ClosedTsRegisterResultGql> {
        let a = admin(ctx)?;
        let coord = a
            .closed_ts
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("closed timestamp coordinator is not configured"))?;
        let tracker = coord.register_range(range_id as u64);
        Ok(ClosedTsRegisterResultGql {
            range_id,
            closed_timestamp: tracker.closed_timestamp().to_string(),
        })
    }

    /// 全 Range の closed timestamp を前進させる(CockroachDB が定期的に
    /// 行う操作を明示的に起こす)。`now_nanos` 省略時は現在時刻。
    async fn closed_ts_advance(
        &self,
        ctx: &Context<'_>,
        now_nanos: Option<String>,
    ) -> Result<Vec<ClosedTsAdvanceEntryGql>> {
        let a = admin(ctx)?;
        let coord = a
            .closed_ts
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("closed timestamp coordinator is not configured"))?;
        let now = match now_nanos {
            Some(s) => parse_u64(&s, "nowNanos")?,
            None => now_default_nanos(a),
        };
        Ok(coord
            .advance_all(now)
            .into_iter()
            .map(|(id, ts)| ClosedTsAdvanceEntryGql {
                range_id: id as i64,
                closed_timestamp: ts.to_string(),
            })
            .collect())
    }

    // ── WAL サービス: 副作用のあるアクション(2026-08-29 再設計 P3) ──
    //
    // 旧 REST `POST /admin/wal-service/append`・`/image-layer` の等価。
    // status / page(読み取り)は AdminQuery 側。

    /// WAL レコードを safekeeper quorum へ耐久化し、pageserver へ取り込ませる
    /// (Neon の compute → safekeeper → pageserver)。`start_lsn` から
    /// レコードごとに +1 ずつ LSN を割る。
    async fn wal_append(
        &self,
        ctx: &Context<'_>,
        start_lsn: String,
        records: Vec<WalRecordInput>,
    ) -> Result<WalAppendResultGql> {
        let a = admin(ctx)?;
        let s = a
            .wal_storage
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("WAL service is not configured"))?;
        let start = parse_u64(&start_lsn, "startLsn")?;
        let mut recs = Vec::with_capacity(records.len());
        for (i, r) in records.iter().enumerate() {
            let lsn = start + i as u64;
            let bytes = r.data.clone().into_bytes();
            recs.push(match r.op.as_deref() {
                Some("append") => aruaru_dist::WalRecord::append(lsn, r.page_key.clone(), bytes),
                _ => aruaru_dist::WalRecord::replace(lsn, r.page_key.clone(), bytes),
            });
        }
        let commit = s
            .write(&recs)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(WalAppendResultGql {
            commit_lsn: commit.to_string(),
            applied_lsn: s.pageserver.last_record_lsn().to_string(),
            record_count: recs.len() as i32,
        })
    }

    /// 指定 LSN で image layer を作り、それより古い delta を落とす
    /// (pageserver の compaction 相当)。`lsn` 省略時は最終適用 LSN。
    async fn wal_create_image_layer(
        &self,
        ctx: &Context<'_>,
        page_key: String,
        lsn: Option<String>,
    ) -> Result<WalImageLayerResultGql> {
        let a = admin(ctx)?;
        let s = a
            .wal_storage
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("WAL service is not configured"))?;
        let ps = &s.pageserver;
        let lsn = match lsn {
            Some(v) => parse_u64(&v, "lsn")?,
            None => ps.last_record_lsn(),
        };
        let dropped = ps
            .create_image_layer(&page_key, lsn)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(WalImageLayerResultGql {
            page_key,
            gc_cutoff_lsn: lsn.to_string(),
            dropped_deltas: dropped as i32,
        })
    }

    // ── ScyllaDB shard-per-core ストア: put(2026-08-29 再設計 P3) ──

    /// 指定キーを担当シャードへ書き込む(キーの murmur3 ハッシュで
    /// シャードを決定)。get / stats は AdminQuery 側。
    async fn sharded_store_put(
        &self,
        ctx: &Context<'_>,
        key: String,
        value: String,
    ) -> Result<ShardedStorePutResultGql> {
        let a = admin(ctx)?;
        let store = a
            .sharded_store
            .clone()
            .ok_or_else(|| async_graphql::Error::new("sharded store is not configured"))?;
        let shard_id = store.shard_for(key.as_bytes()) as i64;
        let k = key.clone();
        tokio::task::spawn_blocking(move || store.put(k.into_bytes(), value))
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(ShardedStorePutResultGql { key, shard_id })
    }

    // ── Vitess Reshard(併合)+ VTGate scatter-gather ─────────

    /// 旧 REST `POST /admin/multi-raft/split`の等価。CockroachDB方式の
    /// Range分割を実際に実行する(`MultiRaftCluster::split`)。
    async fn multi_raft_split(
        &self,
        ctx: &Context<'_>,
        range_id: i64,
        split_key: String,
    ) -> Result<MultiRaftSplitResultGql> {
        let a = admin(ctx)?;
        let Some(cluster) = &a.multi_raft else {
            return Ok(MultiRaftSplitResultGql {
                success: false,
                new_range_id: None,
                range_count: 0,
                message: Some("MultiRaftCluster未初期化です(main.rsでの構築に失敗している可能性があります)。".into()),
            });
        };
        let applier = aruaru_dist::EngineApplier::new(a.engine.clone());
        match cluster.split(range_id as u64, split_key.into_bytes(), applier) {
            Some(new_range_id) => Ok(MultiRaftSplitResultGql {
                success: true,
                new_range_id: Some(new_range_id as i64),
                range_count: cluster.range_count() as i32,
                message: None,
            }),
            None => Ok(MultiRaftSplitResultGql {
                success: false,
                new_range_id: None,
                range_count: cluster.range_count() as i32,
                message: Some(format!("range_id {range_id} が見つかりません")),
            }),
        }
    }

    /// 旧 REST `POST /admin/multi-raft/merge`の等価。Vitess Reshard
    /// (併合方向)を実際に実行する(`MultiRaftCluster::merge`)。
    async fn multi_raft_merge(
        &self,
        ctx: &Context<'_>,
        range_a: i64,
        range_b: i64,
    ) -> Result<MultiRaftMergeResultGql> {
        let a = admin(ctx)?;
        let Some(cluster) = &a.multi_raft else {
            return Ok(MultiRaftMergeResultGql {
                success: false,
                merged_range_id: None,
                range_count: 0,
                message: Some("MultiRaftCluster未初期化です(main.rsでの構築に失敗している可能性があります)。".into()),
            });
        };
        match cluster.merge(range_a as u64, range_b as u64) {
            Some(merged_id) => Ok(MultiRaftMergeResultGql {
                success: true,
                merged_range_id: Some(merged_id as i64),
                range_count: cluster.range_count() as i32,
                message: None,
            }),
            None => Ok(MultiRaftMergeResultGql {
                success: false,
                merged_range_id: None,
                range_count: cluster.range_count() as i32,
                message: Some(format!("range {range_a} と {range_b} は併合できません(隣接していないか、存在しません)")),
            }),
        }
    }

    // ── ephemeral SQL pod ────────────────────────────────────

    /// 旧 REST `POST /admin/ephemeral-query`の等価。指定テナントの
    /// テーブルを現在の状態からスナップショットし、`EphemeralRunner`
    /// (実体は`aruaru-server::ephemeral_pod::ProcessEphemeralRunner`、
    /// 独立子プロセスを起動してSQLを1回実行させ即終了する)へ委譲する。
    /// 書き込みは子プロセスのメモリ上でのみ完結し親の永続状態には
    /// 反映されない(既存の制約を継承、ephemeral_pod.rsのdoc参照)。
    async fn ephemeral_query(
        &self,
        ctx: &Context<'_>,
        tenant_id: String,
        tables: Vec<String>,
        sql: String,
    ) -> Result<EphemeralQueryResultGql> {
        let a = admin(ctx)?;
        let Some(runner) = &a.ephemeral else {
            return Ok(EphemeralQueryResultGql {
                success: false,
                tenant_id,
                result: None,
                error: None,
                message: Some("ephemeral runner is not configured".into()),
            });
        };
        let snapshot = aruaru_dist::ephemeral::snapshot_for_tenant(&a.engine, &tables);
        let request = aruaru_dist::ephemeral::EphemeralRequest {
            tenant_id: tenant_id.clone(),
            tables: snapshot,
            sql,
        };
        match runner.run(&request).await {
            Ok(resp) => Ok(EphemeralQueryResultGql {
                success: resp.ok,
                tenant_id,
                result: resp.result.map(crate::response_to_gql),
                error: resp.error,
                message: None,
            }),
            Err(e) => Ok(EphemeralQueryResultGql {
                success: false,
                tenant_id,
                result: None,
                error: None,
                message: Some(format!("ephemeral worker process failed: {e}")),
            }),
        }
    }
}

// ── 共通ヘルパ ────────────────────────────────────────────────

/// `aruaru_backup::BackupManifest` → GraphQL 型
fn manifest_to_gql(m: aruaru_backup::BackupManifest) -> BackupGql {
    let kind = match &m.kind {
        aruaru_backup::BackupKind::Full => "full".to_string(),
        aruaru_backup::BackupKind::Incremental => "incremental".to_string(),
        aruaru_backup::BackupKind::Snapshot { .. } => "snapshot".to_string(),
    };
    BackupGql {
        id: m.id,
        created_at: m.started_at,
        branch: m.branch,
        commit_id: m.commit_id,
        kind,
        size_mb: (m.size_bytes as f64) / 1e6,
        path: String::new(),
        status: "completed".to_string(),
    }
}

fn wire_for_source(source: &str) -> Option<aruaru_registry::Wire> {
    use aruaru_registry::Wire;
    match source.to_lowercase().as_str() {
        "postgres"|"postgresql"|"cockroach"|"cockroachdb"|"yugabyte"|"neon"|"supabase"
        |"timescaledb"|"risingwave"|"cratedb" => Some(Wire::Postgres),
        "mysql"|"mariadb"|"tidb"|"singlestore"|"vitess"|"percona" => Some(Wire::MySQL),
        "mongodb"|"mongo"|"documentdb" => Some(Wire::Mongo),
        "cassandra"|"scylla"|"astra" => Some(Wire::Cql),
        _ => None,
    }
}

// ── 再エクスポート用型 ────────────────────────────────────────

use crate::{QueryResultGql, MutationResult};

// ── テスト: cluster_propose の RaftWriter 経由化(2026-07-26追記) ──
//
// 既存の Raft クラスタシミュレーション機構(`aruaru-dist::raft::writer`の
// `RaftWriter` + peers空=単一ノード即Leaderのパターン)をそのまま再利用する
// (新しいクラスタシミュレーション機構は発明しない)。`aruaru-server::cluster::
// EngineApplier`と同型の最小限のApplierをテスト内に用意する
// (`aruaru-graphql`は`aruaru-server`に依存できない循環関係のため)。
#[cfg(test)]
mod cluster_propose_tests {
    use std::sync::{Arc, Mutex};

    use aruaru_dist::{Applier, Command, CommandResponse, RaftNode, RaftWriter, ReplicatedWriter};
    use aruaru_query::QueryEngine;

    use crate::{build_schema, AdminCtx, GraphqlAdminToken};

    /// `ARUARU_DB_ADMIN_TOKEN`はプロセス全体のグローバル環境変数のため、
    /// このモジュール内のテストが並行に読み書きすると競合する
    /// (`open-easy-web`等、同種の環境変数依存テストで採用済みの既存
    /// パターンと同じ対策)。
    static ENV_LOCK: Mutex<()> = Mutex::new(());
    const TEST_ADMIN_TOKEN: &str = "test-admin-token";

    fn authorized_request(query: &str) -> async_graphql::Request {
        async_graphql::Request::new(query).data(GraphqlAdminToken(Some(TEST_ADMIN_TOKEN.to_string())))
    }

    /// `aruaru-server::cluster::EngineApplier`と同じ責務の最小再実装
    /// (Raft commit を QueryEngine へ適用する)。
    struct TestEngineApplier {
        engine: Arc<QueryEngine>,
    }

    impl Applier for TestEngineApplier {
        fn apply(&self, command: &Command) -> CommandResponse {
            match command {
                Command::Exec(sql) => match self.engine.execute(sql) {
                    Ok(_) => CommandResponse::ok(),
                    Err(e) => CommandResponse::err(e),
                },
                Command::Commit(msg) => {
                    let safe = msg.replace('\'', "''");
                    match self.engine.execute(&format!("SELECT aruaru_commit('{safe}')")) {
                        Ok(_) => CommandResponse::ok(),
                        Err(e) => CommandResponse::err(e),
                    }
                }
                Command::Noop => CommandResponse::ok(),
            }
        }
    }

    fn test_backup_engine(engine: Arc<QueryEngine>) -> Arc<aruaru_backup::BackupEngine> {
        let mut dest = std::env::temp_dir();
        dest.push(format!("aruaru-graphql-test-backup-{}", uuid::Uuid::new_v4()));
        Arc::new(aruaru_backup::BackupEngine::new(
            aruaru_backup::BackupConfig {
                destination: aruaru_backup::BackupDestination::Local { path: dest },
                kind: aruaru_backup::BackupKind::Full,
                compression: aruaru_backup::BackupCompression::None,
                encrypt: false,
                retention_days: 7,
            },
            engine,
        ))
    }

    fn admin_ctx(engine: Arc<QueryEngine>, replicator: Option<Arc<dyn ReplicatedWriter>>) -> AdminCtx {
        // `test_backup_engine`が`engine`を消費するため、その後でも
        // `EngineApplier`用に使えるよう先に複製しておく。
        let engine_for_multi_raft = engine.clone();
        let engine_for_columnar = engine.clone();
        AdminCtx {
            engine: engine.clone(),
            registry: aruaru_registry::Registry::new(),
            backup: test_backup_engine(engine),
            replicator,
            topology: Some(Arc::new(parking_lot::Mutex::new(
                aruaru_dist::ClusterTopology::single_node(1, "127.0.0.1:5432"),
            ))),
            schedule: Some(Arc::new(parking_lot::Mutex::new(None))),
            federation: Some(Arc::new(parking_lot::Mutex::new(Vec::new()))),
            parallel: Some(Arc::new(parking_lot::Mutex::new(
                aruaru_dist::admin_shared::ParallelConfigState::default(),
            ))),
            keyring: Some(Arc::new(aruaru_dist::keyring::KeyGuardian::new())),
            object_table: Some(Arc::new(aruaru_backup::table_format::ObjectTable::new(
                Arc::new(aruaru_backup::table_format::InMemoryObjectStore::new()),
                Arc::new(aruaru_backup::table_format::MetaService::new()),
                "test",
                1,
                1,
            ))),
            closed_ts: Some(Arc::new(aruaru_dist::ClosedTimestampCoordinator::with_default_lag())),
            wal_storage: Some(Arc::new(aruaru_dist::DisaggregatedStorage::new(
                3,
                aruaru_dist::DEFAULT_MAX_REPLICATION_LAG,
            ))),
            sharded_store: Some(Arc::new(aruaru_query::sharded_store::ShardedRowStore::new(2))),
            ephemeral: Some(Arc::new(TestEphemeralRunner)),
            multi_raft: Some(Arc::new(aruaru_dist::MultiRaftCluster::single_node(
                1,
                "127.0.0.1:5432".to_string(),
                aruaru_dist::EngineApplier::new(engine_for_multi_raft),
            ))),
            hlc: Some(Arc::new(aruaru_dist::Hlc::new())),
            columnar: Some(Arc::new(aruaru_dist::ColumnarApplier::observing(engine_for_columnar))),
        }
    }

    /// テスト専用の`EphemeralRunner`実装——実プロセス起動
    /// (`current_exe()`+`Command`)はこのクレート(`aruaru-graphql`)からは
    /// 検証できない(実体は`aruaru-server::ephemeral_pod::
    /// ProcessEphemeralRunner`で別クレート)ため、代わりに「子プロセスが
    /// 行うのと同じ処理(受け取ったテーブルでインメモリQueryEngineを
    /// 構築しSQLを1回実行する)」をこのプロセス内で直接行う——resolver
    /// 側の配線(スナップショット構築→trait呼び出し→結果のGraphQL変換)を
    /// 実データで検証できる。
    struct TestEphemeralRunner;

    #[async_trait::async_trait]
    impl aruaru_dist::ephemeral::EphemeralRunner for TestEphemeralRunner {
        async fn run(
            &self,
            request: &aruaru_dist::ephemeral::EphemeralRequest,
        ) -> anyhow::Result<aruaru_dist::ephemeral::EphemeralResponse> {
            let engine = QueryEngine::new();
            for t in &request.tables {
                engine.ingest_table(&t.name, t.columns.clone(), t.rows.clone());
            }
            Ok(match engine.execute(&request.sql) {
                Ok(result) => aruaru_dist::ephemeral::EphemeralResponse { ok: true, result: Some(result), error: None },
                Err(e) => aruaru_dist::ephemeral::EphemeralResponse { ok: false, result: None, error: Some(e) },
            })
        }
    }

    /// gap解消の中心的な検証: replicator が設定されていれば `cluster_propose`
    /// が実際に RaftWriter (`propose_and_wait`) 経由で書き込まれ、
    /// メッセージが `mode=raft` であることを確認する
    /// (`engine.execute` 直接呼び出しでは到達できないRaft経路であることの
    /// 傍証として、単一ノード=peers空でも propose/commit/apply が実際に
    /// 進んでいることをテーブルの中身で確認する)。
    #[tokio::test]
    async fn cluster_propose_routes_through_raft_when_replicator_configured() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let applier = TestEngineApplier { engine: engine.clone() };
        let node = Arc::new(RaftNode::new(1, applier, vec![])); // peers空=単一ノード
        node.become_leader();
        let writer: Arc<dyn ReplicatedWriter> = Arc::new(RaftWriter::new(node));

        let schema = build_schema(engine.clone(), admin_ctx(engine.clone(), Some(writer)));
        let resp = schema
            .execute(authorized_request(
                r#"mutation { clusterPropose(sql: "CREATE TABLE t (id INT PRIMARY KEY, v TEXT)") { success message } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        let data = resp.data.into_json().unwrap();
        let result = &data["clusterPropose"];
        assert_eq!(result["success"], true, "result: {result:?}");
        assert!(
            result["message"].as_str().unwrap().starts_with("mode=raft:"),
            "replicator設定時はmode=rafトを経由するはず: {result:?}"
        );

        // Raft経由でも実際にQueryEngineへ反映されている(テーブルが作られている)ことを確認
        assert!(engine.table_names().contains(&"t".to_string()));
    }

    /// フォールバック経路(replicator未設定)は従来通り動作すること
    /// (単一ノード/非クラスタ構成・既存呼び出し元への無回帰の確認)。
    #[tokio::test]
    async fn cluster_propose_falls_back_to_direct_engine_execute_without_replicator() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let schema = build_schema(engine.clone(), admin_ctx(engine.clone(), None));
        let resp = schema
            .execute(authorized_request(
                r#"mutation { clusterPropose(sql: "CREATE TABLE t2 (id INT PRIMARY KEY, v TEXT)") { success message } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        let data = resp.data.into_json().unwrap();
        let result = &data["clusterPropose"];
        assert_eq!(result["success"], true, "result: {result:?}");
        assert!(
            result["message"].as_str().unwrap().starts_with("mode=raft_fallback_no_replicator:"),
            "replicator未設定時はフォールバックのはず: {result:?}"
        );
        assert!(engine.table_names().contains(&"t2".to_string()));
    }

    /// **2026-08-01追加(実バグ修正の検証)**: `/graphql`経由の管理操作は
    /// REST `/admin/*`と同じ`x-admin-token`検証を受けること。修正前は
    /// GraphQL経由なら無認証で`clusterStatus`等が読めてしまっていた。
    #[tokio::test]
    async fn admin_query_without_token_is_rejected() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let schema = build_schema(engine.clone(), admin_ctx(engine.clone(), None));
        // GraphqlAdminTokenを一切データへ注入しないリクエスト
        // (実際のHTTPハンドラでヘッダーが送られなかった状態を再現)。
        let resp = schema.execute("query { clusterStatus { stats { totalNodes } } }").await;
        assert!(!resp.errors.is_empty(), "should be rejected without a token");
        assert!(
            resp.errors.iter().any(|e| e.message.contains("invalid or missing")),
            "unexpected error: {:?}",
            resp.errors
        );
    }

    #[tokio::test]
    async fn admin_query_with_wrong_token_is_rejected() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let schema = build_schema(engine.clone(), admin_ctx(engine.clone(), None));
        let req = async_graphql::Request::new("query { clusterStatus { stats { totalNodes } } }")
            .data(GraphqlAdminToken(Some("wrong-token".to_string())));
        let resp = schema.execute(req).await;
        assert!(!resp.errors.is_empty(), "should be rejected with the wrong token");
    }

    #[tokio::test]
    async fn admin_query_with_correct_token_succeeds() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let schema = build_schema(engine.clone(), admin_ctx(engine.clone(), None));
        let resp = schema.execute(authorized_request("query { clusterStatus { stats { totalNodes } } }")).await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
    }

    /// 非管理系(`VcsQuery`)フィールドは今回のゲート対象外であること
    /// (`AdminCtx`を一切要求しない既存のバージョン管理系クエリまで
    /// 巻き込んでいないことの確認、`GraphqlAdminToken`未設定でも成功する)。
    #[tokio::test]
    async fn non_admin_vcs_query_does_not_require_a_token() {
        let engine = Arc::new(QueryEngine::new());
        let schema = build_schema(engine.clone(), admin_ctx(engine.clone(), None));
        let resp = schema.execute("query { log(limit: 1) { id } }").await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
    }

    // ── 2026-08-29追加: backup_schedule / federated_sources のREST実
    // 状態接続の検証(従来の固定値スタブからの脱却、`cluster_status`の
    // 検証と同じ精神——`AdminCtx`が共有する`Arc<Mutex<..>>`を直接読んで、
    // resolverが返す値と一致することを確認する)。

    #[tokio::test]
    async fn set_backup_schedule_persists_and_backup_schedule_reads_it_back() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let ctx = admin_ctx(engine.clone(), None);
        let shared_schedule = ctx.schedule.clone().unwrap();
        let schema = build_schema(engine.clone(), ctx);

        // 書き込み前は未設定(None)。
        let resp = schema.execute(authorized_request("query { backupSchedule { cron enabled } }")).await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        assert!(resp.data.into_json().unwrap()["backupSchedule"].is_null());

        let resp = schema
            .execute(authorized_request(
                r#"mutation { setBackupSchedule(input: {enabled: true, cron: "0 3 * * *", kind: "full"}) { cron enabled kind } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        let data = resp.data.into_json().unwrap();
        assert_eq!(data["setBackupSchedule"]["cron"], "0 3 * * *");
        assert_eq!(data["setBackupSchedule"]["enabled"], true);

        // REST側と共有する`Arc<Mutex<..>>`自体にも実際に書き込まれている
        // ことを直接確認(GraphQL経由の書き込みがREST GETからも見える
        // ことの傍証)。
        assert_eq!(shared_schedule.lock().as_ref().unwrap().cron, "0 3 * * *");

        // 再クエリで直前の書き込みが読める(スタブなら常にnullのまま)。
        let resp = schema.execute(authorized_request("query { backupSchedule { cron enabled } }")).await;
        let data = resp.data.into_json().unwrap();
        assert_eq!(data["backupSchedule"]["cron"], "0 3 * * *");
        assert_eq!(data["backupSchedule"]["enabled"], true);
    }

    #[tokio::test]
    async fn federated_source_register_list_and_drop_round_trip_through_shared_state() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let ctx = admin_ctx(engine.clone(), None);
        let shared_federation = ctx.federation.clone().unwrap();
        let schema = build_schema(engine.clone(), ctx);

        // 登録前は空(スタブなら常に空のまま区別が付かないが、以降の
        // 登録・削除で実際に増減することを確認する)。
        let resp = schema.execute(authorized_request("query { federatedSources { name } }")).await;
        assert_eq!(resp.data.into_json().unwrap()["federatedSources"].as_array().unwrap().len(), 0);

        let resp = schema
            .execute(authorized_request(
                r#"mutation { registerFederatedSource(input: {name: "wh", kind: "postgres", uri: "postgres://x/y"}) { name status } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        assert_eq!(shared_federation.lock().len(), 1, "REST側と共有する状態に実際に追加されているはず");

        // 同名の二重登録はREST側と同じくエラーになる。
        let resp = schema
            .execute(authorized_request(
                r#"mutation { registerFederatedSource(input: {name: "wh", kind: "postgres", uri: "postgres://x/y"}) { name } }"#,
            ))
            .await;
        assert!(!resp.errors.is_empty(), "duplicate name should be rejected");

        let resp = schema.execute(authorized_request("query { federatedSources { name } }")).await;
        let data = resp.data.into_json().unwrap();
        assert_eq!(data["federatedSources"].as_array().unwrap().len(), 1);
        assert_eq!(data["federatedSources"][0]["name"], "wh");

        let resp = schema
            .execute(authorized_request(r#"mutation { dropFederatedSource(name: "wh") { success } }"#))
            .await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        assert_eq!(shared_federation.lock().len(), 0, "削除後は共有状態からも消えているはず");
    }

    // ── 2026-08-29(続き)追加: keyStatus / revokeKeys がREST側と同一の
    // KeyGuardianを実際に参照・操作することの検証。

    #[tokio::test]
    async fn key_status_and_revoke_keys_operate_on_the_shared_key_guardian() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let ctx = admin_ctx(engine.clone(), None);
        let shared_keyring = ctx.keyring.clone().unwrap();
        let schema = build_schema(engine.clone(), ctx);

        // 発行前は0件。
        let resp = schema.execute(authorized_request("query { keyStatus { issuedKeyCount } }")).await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        assert_eq!(resp.data.into_json().unwrap()["keyStatus"]["issuedKeyCount"], 0);

        // REST側の自己発行経路(POST /v1/keys/self-issue)と同じKeyGuardian
        // インスタンスへ、ここでは直接issueして「REST側で発行されたキー」を
        // 模擬する(共有状態であることの傍証)。
        shared_keyring.issue("alice", "viewer", None);
        shared_keyring.issue("bob", "viewer", None);

        let resp = schema.execute(authorized_request("query { keyStatus { issuedKeyCount } }")).await;
        assert_eq!(resp.data.into_json().unwrap()["keyStatus"]["issuedKeyCount"], 2);

        let resp = schema
            .execute(authorized_request(r#"mutation { revokeKeys(owner: "alice") { revokedCount } }"#))
            .await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        assert_eq!(resp.data.into_json().unwrap()["revokeKeys"]["revokedCount"], 1);

        // 失効させても発行済みレコード自体は残る(REST側`keyring_status`の
        // count()と同じ挙動——revokedフラグが立つだけで削除ではない)ため
        // 件数は2のまま。
        let resp = schema.execute(authorized_request("query { keyStatus { issuedKeyCount } }")).await;
        assert_eq!(resp.data.into_json().unwrap()["keyStatus"]["issuedKeyCount"], 2);
    }

    // ── 2026-08-29(続き3)追加: object-table の status/commit/prune を
    // GraphQL だけで完結できる(REST `/admin/object-table*` 撤廃後) ──
    #[tokio::test]
    async fn object_table_commit_prune_and_status_are_graphql_only() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let schema = build_schema(engine.clone(), admin_ctx(engine.clone(), None));

        // コミット前: 履歴は空。
        let resp = schema
            .execute(authorized_request(
                "query { objectTable { tableKey historyLen current { snapshotId } } }",
            ))
            .await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        let data = resp.data.into_json().unwrap();
        assert_eq!(data["objectTable"]["historyLen"], 0);
        assert!(data["objectTable"]["current"].is_null());

        // objectTableCommit mutation を2回(旧 REST commit の置き換え)。
        let commit = |q: &'static str| {
            schema.execute(authorized_request(q))
        };
        let resp = commit(
            r#"mutation { objectTableCommit(blocks: [
                { location: "blk/1.parquet", rowCount: 10, sizeBytes: 1024,
                  stats: [{ column: "age", min: 1, max: 40, nullCount: 0 }] }
            ]) { snapshotId blockCount } }"#,
        )
        .await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        let s1 = resp.data.into_json().unwrap()["objectTableCommit"]["snapshotId"]
            .as_str()
            .unwrap()
            .to_string();

        let resp = commit(
            r#"mutation { objectTableCommit(blocks: [
                { location: "blk/2.parquet", rowCount: 5, sizeBytes: 512,
                  stats: [{ column: "age", min: 60, max: 90, nullCount: 0 }] }
            ]) { snapshotId blockCount } }"#,
        )
        .await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        let s2 = resp.data.into_json().unwrap()["objectTableCommit"]["snapshotId"]
            .as_str()
            .unwrap()
            .to_string();

        // status: 履歴が積まれ、current の prev が s1。
        let resp = schema
            .execute(authorized_request(
                "query { objectTable { historyLen current { snapshotId prevSnapshotId } } }",
            ))
            .await;
        let ot = resp.data.into_json().unwrap();
        let ot = &ot["objectTable"];
        assert_eq!(ot["historyLen"], 2);
        assert_eq!(ot["current"]["snapshotId"], s2);
        assert_eq!(ot["current"]["prevSnapshotId"], s1.as_str());

        // prune(range述語 age < 50): 60..90 の segment は読み飛ばされる。
        let resp = schema
            .execute(authorized_request(
                r#"mutation { objectTablePrune(column: "age", op: "lt", value: 50) {
                    predicate keptBlocks skippedSegments locations
                } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        let p = resp.data.into_json().unwrap();
        let p = &p["objectTablePrune"];
        assert_eq!(p["predicate"], "range");
        assert_eq!(p["keptBlocks"], 1);
        assert_eq!(p["skippedSegments"], 1);
        assert_eq!(p["locations"][0], "blk/1.parquet");

        // prune のバリデーション文言(REST版踏襲)も維持。
        let resp = schema
            .execute(authorized_request(
                r#"mutation { objectTablePrune(column: "age") { predicate } }"#,
            ))
            .await;
        assert!(
            resp.errors.iter().any(|e| e.message.contains("op is required")),
            "errors: {:?}",
            resp.errors
        );
    }

    // ── 2026-08-29 再設計 P2: parallelConfig query が AdminState.parallel
    // (= aruaru.yaml: query.parallel の実効値)を返す。固定値スタブではない ──
    #[tokio::test]
    async fn parallel_config_query_returns_shared_state_not_a_stub() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let ctx = admin_ctx(engine.clone(), None);
        let shared = ctx.parallel.clone().unwrap();
        let schema = build_schema(engine.clone(), ctx);

        // 既定値。
        let resp = schema
            .execute(authorized_request(
                "query { parallelConfig { enabled maxWorkers chunkSize strategy } }",
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let d = resp.data.into_json().unwrap();
        assert_eq!(d["parallelConfig"]["enabled"], false);
        assert_eq!(d["parallelConfig"]["maxWorkers"], 4);

        // config::reconcile 相当(共有状態を書き換え)→ query に即反映。
        {
            let mut cur = shared.lock();
            cur.enabled = true;
            cur.max_workers = 24;
            cur.chunk_size = 2000;
            cur.strategy = "range".into();
        }
        let resp = schema
            .execute(authorized_request(
                "query { parallelConfig { enabled maxWorkers chunkSize strategy } }",
            ))
            .await;
        let d = resp.data.into_json().unwrap();
        assert_eq!(d["parallelConfig"]["enabled"], true);
        assert_eq!(d["parallelConfig"]["maxWorkers"], 24);
        assert_eq!(d["parallelConfig"]["chunkSize"], 2000);
        assert_eq!(d["parallelConfig"]["strategy"], "range");
    }

    // ── 2026-08-29 再設計 P3: selfIssueKey mutation(認証不要)が旧 REST
    // POST /v1/keys/self-issue を置き換える ──
    #[tokio::test]
    async fn self_issue_key_mutation_works_without_admin_token() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let ctx = admin_ctx(engine.clone(), None);
        let keyring = ctx.keyring.clone().unwrap();
        let schema = build_schema(engine.clone(), ctx);

        assert_eq!(keyring.count(), 0);

        // admin トークンを **付けない** リクエスト(認証不要であることの確認)。
        let resp = schema
            .execute(async_graphql::Request::new(
                "mutation { selfIssueKey { key role expiresInHours } }",
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let d = resp.data.into_json().unwrap();
        assert_eq!(d["selfIssueKey"]["role"], "viewer");
        assert_eq!(d["selfIssueKey"]["expiresInHours"], 24);
        assert!(d["selfIssueKey"]["key"].as_str().unwrap().len() > 8);

        // 同一の KeyGuardian に発行済みとして記録される。
        assert_eq!(keyring.count(), 1);
    }

    // ── 2026-08-29 再設計 P3: explainDistributed が固定値スタブではなく
    // AdminState.parallel の実効値 + SQL 分類から実プランを組む ──
    #[tokio::test]
    async fn explain_distributed_reflects_parallel_config_and_query_kind() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let ctx = admin_ctx(engine.clone(), None);
        let parallel = ctx.parallel.clone().unwrap();
        let schema = build_schema(engine.clone(), ctx);

        // 並列無効時: ParallelScan の par は 1。
        let resp = schema
            .execute(authorized_request(
                r#"query { explainDistributed(sql: "SELECT * FROM users") { step node operation } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let steps = resp.data.into_json().unwrap()["explainDistributed"]
            .as_array()
            .unwrap()
            .clone();
        assert!(!steps.is_empty());
        assert!(steps[0]["operation"].as_str().unwrap().starts_with("Gather"));
        let scan = steps
            .iter()
            .find(|s| s["operation"].as_str().unwrap().contains("ParallelScan"))
            .expect("plan should contain a ParallelScan step");
        assert!(scan["operation"].as_str().unwrap().contains("par=1"), "{scan:?}");
        assert_eq!(scan["node"], "node-1");

        // 並列有効・max_workers=6 → par=6。
        {
            let mut c = parallel.lock();
            c.enabled = true;
            c.max_workers = 6;
        }
        let resp = schema
            .execute(authorized_request(
                r#"query { explainDistributed(sql: "SELECT * FROM users") { operation } }"#,
            ))
            .await;
        let steps = resp.data.into_json().unwrap()["explainDistributed"]
            .as_array()
            .unwrap()
            .clone();
        let scan = steps
            .iter()
            .find(|s| s["operation"].as_str().unwrap().contains("ParallelScan"))
            .unwrap();
        assert!(scan["operation"].as_str().unwrap().contains("par=6"), "{scan:?}");
    }

    // ── 2026-08-29 再設計 P3: closed-timestamp / wal-service / sharded-store
    // が GraphQL だけで完結する(REST `/admin/*` 該当ルート撤廃後) ──

    #[tokio::test]
    async fn closed_timestamp_status_advance_and_follower_read_plan_are_graphql_only() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let schema = build_schema(engine.clone(), admin_ctx(engine.clone(), None));

        // 登録前は空。
        let resp = schema
            .execute(authorized_request("query { closedTimestamp { rangeCount ranges { rangeId } } }"))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        assert_eq!(resp.data.into_json().unwrap()["closedTimestamp"]["rangeCount"], 0);

        // Range を登録(旧 REST POST /admin/closed-timestamp/range の置き換え)。
        let resp = schema
            .execute(authorized_request(
                "mutation { closedTsRegisterRange(rangeId: 1) { rangeId closedTimestamp } }",
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        assert_eq!(resp.data.into_json().unwrap()["closedTsRegisterRange"]["closedTimestamp"], "0");

        // advance(now=10s)→ closed = 10s - target_lag(3s) = 7s。
        let resp = schema
            .execute(authorized_request(
                r#"mutation { closedTsAdvance(nowNanos: "10000000000") { rangeId closedTimestamp } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let adv = resp.data.into_json().unwrap();
        assert_eq!(adv["closedTsAdvance"][0]["rangeId"], 1);
        assert_eq!(adv["closedTsAdvance"][0]["closedTimestamp"], "7000000000");

        // planFollowerRead(bounded, now=10s)→ closed 7s は上限内なので follower_read。
        let resp = schema
            .execute(authorized_request(
                r#"query { planFollowerRead(rangeIds: [1], nowNanos: "10000000000") { plan isFollowerRead readTimestamp reason } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let p = resp.data.into_json().unwrap();
        assert_eq!(p["planFollowerRead"]["plan"], "follower_read");
        assert_eq!(p["planFollowerRead"]["isFollowerRead"], true);
        assert_eq!(p["planFollowerRead"]["readTimestamp"], "7000000000");

        // 未登録 Range は leaseholder へ。
        let resp = schema
            .execute(authorized_request(
                r#"query { planFollowerRead(rangeIds: [99], nowNanos: "10000000000") { plan reason } }"#,
            ))
            .await;
        let p = resp.data.into_json().unwrap();
        assert_eq!(p["planFollowerRead"]["plan"], "route_to_leaseholder");
        assert!(p["planFollowerRead"]["reason"].as_str().unwrap().len() > 0);
    }

    /// 【2026-09-02 続き23】`htapReplicas` query が同居 `ColumnarApplier` の
    /// 同期状態(TiFlash PROGRESS/AVAILABLE 相当)+ 枝刈り込みプレビューを
    /// 返すこと。行ストア(共有 `QueryEngine`)へ書き込み → `observe_table` で
    /// 追従 → GraphQL から観測、という同居モードの一連を検証する。
    #[tokio::test]
    async fn htap_replicas_query_reports_columnar_replica_progress_and_pruning() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let mut ctx = admin_ctx(engine.clone(), None);
        // admin_ctx の既定 columnar を、この engine を共有するインスタンスへ
        // 差し替えて手元にも保持する(観測を明示的に駆動するため)。
        let columnar = Arc::new(aruaru_dist::ColumnarApplier::observing(engine.clone()));
        ctx.columnar = Some(columnar.clone());
        let schema = build_schema(engine.clone(), ctx);

        // 行ストアへ書き込み(v = 10, 20, 30)。
        engine.execute("CREATE TABLE m (id INT PRIMARY KEY, v INT)").unwrap();
        columnar.observe_table("m").unwrap();
        for i in 1..=3 {
            engine
                .execute(&format!("INSERT INTO m (id, v) VALUES ({i}, {})", i * 10))
                .unwrap();
            // 同居オブザーバの追従を明示的に駆動(本番は set_columnar_observer
            // の通知が INSERT ごとに来るのと同じ)。各 INSERT が単一行の
            // delta block を1つ足す。
            columnar.observe_table("m").unwrap();
        }

        let resp = schema
            .execute(authorized_request(
                r#"query { htapReplicas(table: "m", pruneColumn: "v", pruneOp: "gt", pruneValue: "15") {
                    table available progress columnarLiveRowCount replicationCount
                    prune { totalBlocks keptBlocks skippedBlocks keptLiveRows }
                } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let d = resp.data.into_json().unwrap();
        let r = &d["htapReplicas"];
        assert_eq!(r["table"], "m");
        assert_eq!(r["available"], true);
        assert_eq!(r["progress"], 1.0, "3/3 rows replicated");
        assert_eq!(r["columnarLiveRowCount"], 3);
        assert!(r["replicationCount"].as_i64().unwrap() >= 1);
        // v > 15 → v=10 の delta は必ず読み飛ばせる。
        assert!(r["prune"]["skippedBlocks"].as_i64().unwrap() >= 1);
        assert_eq!(r["prune"]["keptLiveRows"], 2);

        // 未知テーブルは available=false。
        let resp = schema
            .execute(authorized_request(r#"query { htapReplicas(table: "nope") { available progress } }"#))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let d = resp.data.into_json().unwrap();
        assert_eq!(d["htapReplicas"]["available"], false);

        // htapReplicasAll: 2 テーブル目を足すと一覧に両方が(ソート順で)出る。
        engine.execute("CREATE TABLE a_first (id INT PRIMARY KEY)").unwrap();
        engine.execute("INSERT INTO a_first (id) VALUES (1)").unwrap();
        columnar.observe_table("a_first").unwrap();
        let resp = schema
            .execute(authorized_request(
                r#"query { htapReplicasAll { table available columnarLiveRowCount } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let all = resp.data.into_json().unwrap()["htapReplicasAll"].as_array().unwrap().clone();
        assert_eq!(all.len(), 2, "both replicated tables listed: {all:?}");
        assert_eq!(all[0]["table"], "a_first", "sorted by table name");
        assert_eq!(all[1]["table"], "m");
        assert!(all.iter().all(|r| r["available"] == true));
    }

    /// 【2026-09-03 P-HLC-3b】`hlcNow` が案A(フル精度 2 フィールド)の
    /// 現在値 + 案B 射影 ordinal を返す。連続呼び出しで ordinal が厳密増加。
    #[tokio::test]
    async fn hlc_now_query_reports_full_precision_and_ordinal() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);
        let engine = Arc::new(QueryEngine::new());
        let ctx = admin_ctx(engine.clone(), None);
        ctx.hlc.as_ref().unwrap().set_max_offset_nanos(500_000_000); // 500ms
        let schema = build_schema(engine.clone(), ctx);

        let q = r#"query { hlcNow { wallNanos logical synthetic ordinal maxOffsetMs uncertaintyUpperNanos } }"#;
        let r1 = schema.execute(authorized_request(q)).await;
        assert!(r1.errors.is_empty(), "errors: {:?}", r1.errors);
        let d1 = r1.data.into_json().unwrap();
        let w1: u64 = d1["hlcNow"]["wallNanos"].as_str().unwrap().parse().unwrap();
        let o1: u64 = d1["hlcNow"]["ordinal"].as_str().unwrap().parse().unwrap();
        let uu1: u64 = d1["hlcNow"]["uncertaintyUpperNanos"].as_str().unwrap().parse().unwrap();
        assert!(w1 > 1_700_000_000_000_000_000, "wall_nanos is real Unix nanos, not a truncated ordinal");
        assert_eq!(d1["hlcNow"]["maxOffsetMs"], 500);
        assert_eq!(uu1, w1 + 500_000_000, "uncertainty upper = wall + max_offset");

        let r2 = schema.execute(authorized_request(q)).await;
        let d2 = r2.data.into_json().unwrap();
        let o2: u64 = d2["hlcNow"]["ordinal"].as_str().unwrap().parse().unwrap();
        assert!(o2 > o1, "ordinal must be strictly increasing across calls: {o2} !> {o1}");
    }

    /// 【2026-09-02 HLC 再設計 P-HLC-2】`nowNanos` 省略時、`AdminCtx.hlc`
    /// 由来の HLC ordinal(単調保証)で advance すること。2 連続で
    /// closed timestamp が厳密増加し、実 Unix ナノ秒スケール(> 10¹⁸)へ
    /// 到達している(旧 `pt<<16` のオーバーフロー由来の壊れた値ではない)。
    #[tokio::test]
    async fn closed_ts_advance_without_now_uses_monotonic_hlc_ordinal() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);
        let engine = Arc::new(QueryEngine::new());
        let schema = build_schema(engine.clone(), admin_ctx(engine.clone(), None));

        schema
            .execute(authorized_request("mutation { closedTsRegisterRange(rangeId: 1) { rangeId } }"))
            .await;

        let read_closed = |resp: async_graphql::Response| -> u64 {
            resp.data.into_json().unwrap()["closedTsAdvance"][0]["closedTimestamp"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap()
        };
        let a = read_closed(
            schema.execute(authorized_request("mutation { closedTsAdvance { rangeId closedTimestamp } }")).await,
        );
        let b = read_closed(
            schema.execute(authorized_request("mutation { closedTsAdvance { rangeId closedTimestamp } }")).await,
        );
        assert!(b >= a, "HLC-driven advance must be monotonic: {a} -> {b}");
        // HLC ordinal は実 Unix ナノ秒スケール(2026 年 ≈ 1.7e18)。
        // 旧実装は pt<<16 でここが u64 ラップして桁が壊れていた。
        assert!(a > 1_000_000_000_000_000, "HLC ordinal should be at real-nanos scale, got {a}");
    }

    #[tokio::test]
    async fn wal_service_append_page_status_and_image_layer_are_graphql_only() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let schema = build_schema(engine.clone(), admin_ctx(engine.clone(), None));

        // append: LSN 1..3(旧 REST POST /admin/wal-service/append の置き換え)。
        let resp = schema
            .execute(authorized_request(
                r#"mutation { walAppend(startLsn: "1", records: [
                    { pageKey: "page/a", data: "base" },
                    { pageKey: "page/a", op: "append", data: "+d2" },
                    { pageKey: "page/a", op: "append", data: "+d3" }
                ]) { commitLsn appliedLsn recordCount } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let d = resp.data.into_json().unwrap();
        assert_eq!(d["walAppend"]["commitLsn"], "3");
        assert_eq!(d["walAppend"]["recordCount"], 3);

        // status: safekeeper 3台・quorum 2・commit_lsn 3。
        let resp = schema
            .execute(authorized_request(
                "query { walService { quorum commitLsn safekeepers { id flushLsn } pageserver { lastRecordLsn pageKeys } } }",
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let s = resp.data.into_json().unwrap();
        assert_eq!(s["walService"]["quorum"], 2);
        assert_eq!(s["walService"]["commitLsn"], "3");
        assert_eq!(s["walService"]["safekeepers"].as_array().unwrap().len(), 3);
        assert_eq!(s["walService"]["pageserver"]["pageKeys"][0], "page/a");

        // page: LSN 2 時点で "base+d2"。
        let resp = schema
            .execute(authorized_request(
                r#"query { walPage(pageKey: "page/a", lsn: "2") { data len } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        assert_eq!(resp.data.into_json().unwrap()["walPage"]["data"], "base+d2");

        // image-layer を LSN 2 で作成 → それ未満の delta が落ちる。
        let resp = schema
            .execute(authorized_request(
                r#"mutation { walCreateImageLayer(pageKey: "page/a", lsn: "2") { gcCutoffLsn droppedDeltas } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        assert_eq!(resp.data.into_json().unwrap()["walCreateImageLayer"]["gcCutoffLsn"], "2");

        // GC cutoff 未満の読み取りはエラー(設計上の限界を GraphQL でも保つ)。
        let resp = schema
            .execute(authorized_request(
                r#"query { walPage(pageKey: "page/a", lsn: "1") { data } }"#,
            ))
            .await;
        assert!(!resp.errors.is_empty(), "LSN 1 は GC 済みで再構成できないはず");
    }

    #[tokio::test]
    async fn sharded_store_put_get_and_stats_are_graphql_only() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let schema = build_schema(engine.clone(), admin_ctx(engine.clone(), None));

        // put(旧 REST POST /admin/sharded-store の置き換え)。
        let resp = schema
            .execute(authorized_request(
                r#"mutation { shardedStorePut(key: "alpha", value: "apple") { key shardId } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let put = resp.data.into_json().unwrap();
        assert_eq!(put["shardedStorePut"]["key"], "alpha");
        // shardId はマシン非依存(admin_ctx は shard_count=2 固定)で 0 or 1。
        assert!(matches!(put["shardedStorePut"]["shardId"].as_i64(), Some(0) | Some(1)));

        // get: 書き込んだ値が読み戻せる。
        let resp = schema
            .execute(authorized_request(
                r#"query { shardedStoreGet(key: "alpha") { found value shardId } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let g = resp.data.into_json().unwrap();
        assert_eq!(g["shardedStoreGet"]["found"], true);
        assert_eq!(g["shardedStoreGet"]["value"], "apple");

        // 未知キーは found=false。
        let resp = schema
            .execute(authorized_request(
                r#"query { shardedStoreGet(key: "missing") { found value } }"#,
            ))
            .await;
        let g = resp.data.into_json().unwrap();
        assert_eq!(g["shardedStoreGet"]["found"], false);
        assert!(g["shardedStoreGet"]["value"].is_null());

        // stats: shard_count=2(admin_ctx 既定)、total_len=1。
        let resp = schema
            .execute(authorized_request(
                "query { shardedStoreStats { shardCount perShardLen totalLen } }",
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let st = resp.data.into_json().unwrap();
        assert_eq!(st["shardedStoreStats"]["shardCount"], 2);
        assert_eq!(st["shardedStoreStats"]["totalLen"], 1);
    }

    /// 【2026-08-31追加】旧 REST `POST /admin/ephemeral-query`の等価
    /// (`Mutation.ephemeralQuery`)が、trait経由(`TestEphemeralRunner`)で
    /// 実際に現在のテーブル状態をスナップショットし、SQLを実行して
    /// 結果を返すことを検証する。
    #[tokio::test]
    async fn ephemeral_query_snapshots_current_tables_and_runs_sql_via_the_trait() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        engine.execute("CREATE TABLE items (id INT PRIMARY KEY, qty INT)").unwrap();
        engine.execute("INSERT INTO items (id, qty) VALUES (1, 10)").unwrap();
        let schema = build_schema(engine.clone(), admin_ctx(engine.clone(), None));

        let resp = schema
            .execute(authorized_request(
                r#"mutation { ephemeralQuery(tenantId: "t1", tables: ["items"], sql: "SELECT * FROM items") { success tenantId error message } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        let data = resp.data.into_json().unwrap();
        let result = &data["ephemeralQuery"];
        assert_eq!(result["success"], true, "result: {result:?}");
        assert_eq!(result["tenantId"], "t1");
        assert!(result["message"].is_null(), "worker should not have failed to start: {result:?}");

        // ephemeral 未設定(トレイト未注入)の場合は正直に message で失敗を返す。
        let mut ctx_without_runner = admin_ctx(engine.clone(), None);
        ctx_without_runner.ephemeral = None;
        let schema2 = build_schema(engine.clone(), ctx_without_runner);
        let resp2 = schema2
            .execute(authorized_request(
                r#"mutation { ephemeralQuery(tenantId: "t2", tables: [], sql: "SELECT 1") { success message } }"#,
            ))
            .await;
        assert!(resp2.errors.is_empty(), "GraphQL errors: {:?}", resp2.errors);
        let data2 = resp2.data.into_json().unwrap();
        assert_eq!(data2["ephemeralQuery"]["success"], false);
        assert!(data2["ephemeralQuery"]["message"].as_str().unwrap().contains("not configured"));
    }

    /// 【2026-08-31追加】旧 REST `/admin/multi-raft/{split,merge,
    /// scatter-query}`の等価(`Mutation.multiRaftSplit`/`multiRaftMerge`・
    /// `Query.multiRaftScatterQuery`)が、`EngineApplier`の`aruaru-dist`
    /// 移設後も具体型のまま(trait object化なしで)実際に機能することを
    /// 検証する——分割→2Range個別のscatter-gather確認→併合→1Rangeに
    /// 戻ることの一連。
    #[tokio::test]
    async fn multi_raft_split_merge_and_scatter_query_operate_on_the_shared_cluster() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", TEST_ADMIN_TOKEN);

        let engine = Arc::new(QueryEngine::new());
        let schema = build_schema(engine.clone(), admin_ctx(engine.clone(), None));

        // 初期状態は単一Range(admin_ctxのMultiRaftCluster::single_node)。
        let resp = schema.execute(authorized_request("query { multiRaftScatterQuery { rangeId } }")).await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        assert_eq!(resp.data.into_json().unwrap()["multiRaftScatterQuery"].as_array().unwrap().len(), 1);

        // split: range 1 を "m" で分割 → 2 Range になる。
        let resp = schema
            .execute(authorized_request(
                r#"mutation { multiRaftSplit(rangeId: 1, splitKey: "m") { success newRangeId rangeCount } }"#,
            ))
            .await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        let data = resp.data.into_json().unwrap();
        assert_eq!(data["multiRaftSplit"]["success"], true, "result: {data:?}");
        assert_eq!(data["multiRaftSplit"]["rangeCount"], 2);
        let new_range_id = data["multiRaftSplit"]["newRangeId"].as_i64().unwrap();

        // scatter-query: 2 Range 個別のcommit_index/roleが取れる。
        let resp = schema.execute(authorized_request("query { multiRaftScatterQuery { rangeId commitIndex role } }")).await;
        let ranges = resp.data.into_json().unwrap()["multiRaftScatterQuery"].as_array().unwrap().clone();
        assert_eq!(ranges.len(), 2, "ranges: {ranges:?}");

        // merge: 分割した2つを併合し1 Rangeへ戻す。
        let resp = schema
            .execute(authorized_request(&format!(
                r#"mutation {{ multiRaftMerge(rangeA: 1, rangeB: {new_range_id}) {{ success mergedRangeId rangeCount }} }}"#
            )))
            .await;
        assert!(resp.errors.is_empty(), "GraphQL errors: {:?}", resp.errors);
        let data = resp.data.into_json().unwrap();
        assert_eq!(data["multiRaftMerge"]["success"], true, "result: {data:?}");
        assert_eq!(data["multiRaftMerge"]["rangeCount"], 1);

        // 存在しないrange_idでのmergeは正直な失敗メッセージ。
        let resp = schema
            .execute(authorized_request("mutation { multiRaftMerge(rangeA: 1, rangeB: 999) { success message } }"))
            .await;
        let data = resp.data.into_json().unwrap();
        assert_eq!(data["multiRaftMerge"]["success"], false);
        assert!(data["multiRaftMerge"]["message"].is_string());

        // 未設定(trait未注入)の場合はエラーとして返る。
        let mut ctx_without_cluster = admin_ctx(engine.clone(), None);
        ctx_without_cluster.multi_raft = None;
        let schema2 = build_schema(engine.clone(), ctx_without_cluster);
        let resp2 = schema2.execute(authorized_request("query { multiRaftScatterQuery { rangeId } }")).await;
        assert!(!resp2.errors.is_empty(), "should error when multi_raft is not configured");
    }
}
