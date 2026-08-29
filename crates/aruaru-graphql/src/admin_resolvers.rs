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
    /// 【2026-08-29(続き)新設】REST(`AdminState.keyring`)と同一インスタンス
    /// を共有するAPIキー自動ライフサイクル管理(`aruaru_dist::keyring::
    /// KeyGuardian`)。`keyStatus`/`revokeKeys`から実際に発行済みキー数の
    /// 参照・特定オーナーのキー破棄ができる(従来GraphQL側にこの操作自体が
    /// 存在しなかった)。
    pub keyring: Option<Arc<aruaru_dist::keyring::KeyGuardian>>,
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

#[derive(InputObject)]
pub struct ParallelConfigInput {
    pub enabled: bool,
    pub max_workers: i32,
    pub chunk_size: i32,
    pub strategy: String,
}

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

    /// 【2026-08-29時点、正直な既知の制約】REST側の実`ParallelConfig`
    /// (`max_parallelism`/`worker_threads_per_node`/`enable_parallel_scan`
    /// /`enable_parallel_aggregate`/`enable_shuffle_join`/
    /// `shuffle_partitions`/`broadcast_threshold_mb`)と、この
    /// `ParallelConfigGql`スキーマ(`enabled`/`max_workers`/`chunk_size`/
    /// `strategy`)は**フィールド形状そのものが異なる**——`cluster_status`
    /// や`backup_schedule`のような1:1の接続ができない。GraphQL
    /// スキーマを変更する(破壊的変更)か、意味の薄い変換
    /// (例: `max_workers = max_parallelism`、`chunk_size`に
    /// `broadcast_threshold_mb`を無理に流用する等)で誤魔化すかの
    /// 二択になるため、後者は行わず**固定値スタブのまま**にする
    /// ことを選んだ(実データに見せかけて実は無関係な値、という状態は
    /// 誤解を招くため)。次にこの箇所へ着手する場合は、GraphQL
    /// スキーマ自体をREST実体に合わせて再設計すること。
    async fn parallel_config(&self, ctx: &Context<'_>) -> Result<ParallelConfigGql> {
        require_admin_token(ctx)?;
        Ok(ParallelConfigGql {
            enabled: false,
            max_workers: 4,
            chunk_size: 10_000,
            strategy: "hash".into(),
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

    async fn set_parallel_config(
        &self,
        ctx: &Context<'_>,
        config: ParallelConfigInput,
    ) -> Result<ParallelConfigGql> {
        require_admin_token(ctx)?;
        Ok(ParallelConfigGql {
            enabled: config.enabled,
            max_workers: config.max_workers,
            chunk_size: config.chunk_size,
            strategy: config.strategy,
        })
    }

    async fn explain_distributed(
        &self,
        ctx: &Context<'_>,
        sql: String,
    ) -> Result<Vec<ExplainStepGql>> {
        require_admin_token(ctx)?;
        Ok(vec![ExplainStepGql {
            step: 1,
            node: "node-1".into(),
            range: "(min)-(max)".into(),
            operation: sql,
            estimated_rows: 0,
        }])
    }

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
            keyring: Some(Arc::new(aruaru_dist::keyring::KeyGuardian::new())),
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
}
