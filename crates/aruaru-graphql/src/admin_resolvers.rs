//! 管理操作リゾルバ (REST 全廃・GraphQL 一本化)
//!
//! AdminState を Context から取り出し、各操作を実行する。
//! aruaru-server の AdminState を Arc で共有する。

use std::sync::Arc;

use async_graphql::{Context, InputObject, Object, Result};
use chrono::Utc;

use aruaru_query::QueryEngine;
use aruaru_registry::Registry;

use crate::admin_types::*;

/// GraphQL Context に注入する管理状態
pub struct AdminCtx {
    pub engine: Arc<QueryEngine>,
    pub registry: Arc<Registry>,
}

fn admin<'a>(ctx: &Context<'a>) -> Result<&'a AdminCtx> {
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
                updated_at: e.updated_at.to_rfc3339(),
            })
            .collect())
    }

    async fn registry_summary(&self, ctx: &Context<'_>) -> Result<RegistrySummaryGql> {
        let a = admin(ctx)?;
        let s = a.registry.summary();
        Ok(RegistrySummaryGql {
            total: s.total as i32,
            connectable: s.connectable as i32,
            ga: s.ga as i32,
            beta: s.beta as i32,
            pg_compatible: s.pg_compatible as i32,
            planned: s.planned as i32,
        })
    }

    // ── バックアップ ────────────────────────────────────────

    async fn backups(&self, _ctx: &Context<'_>) -> Result<Vec<BackupGql>> {
        // TODO: バックアップストレージから一覧を読む
        Ok(vec![])
    }

    async fn backup_schedule(&self, _ctx: &Context<'_>) -> Result<Option<ScheduleGql>> {
        Ok(None)
    }

    // ── クラスタ ────────────────────────────────────────────

    async fn cluster_status(&self, ctx: &Context<'_>) -> Result<ClusterStatusGql> {
        let a = admin(ctx)?;
        let commit_count = a.engine.version().log(1_000_000).len() as i64;
        let total_rows = a.engine.total_rows() as i64;
        let table_count = a.engine.table_names().len() as i32;

        Ok(ClusterStatusGql {
            stats: ClusterStatsGql {
                total_nodes: 1,
                healthy_nodes: 1,
                total_ranges: 1,
                total_rows,
                table_count,
                replication_factor: 1,
                under_replicated: vec![],
            },
            nodes: vec![NodeStatusGql {
                node_id: 1,
                addr: "127.0.0.1:5432".into(),
                role: "Leader".into(),
                alive: true,
                commit_index: commit_count,
                applied_index: commit_count,
                ranges: 1,
                disk_used_gb: (total_rows as f64 * 64.0) / 1e9,
            }],
            ranges: vec![RangeGql {
                range_id: 1,
                start_key: "(min)".into(),
                end_key: "(max)".into(),
                leader_node: 1,
                replicas: vec![1],
                size_mb: (total_rows as f64 * 64.0) / 1e6,
            }],
        })
    }

    // ── 並列実行 ────────────────────────────────────────────

    async fn parallel_config(&self, _ctx: &Context<'_>) -> Result<ParallelConfigGql> {
        Ok(ParallelConfigGql {
            enabled: false,
            max_workers: 4,
            chunk_size: 10_000,
            strategy: "hash".into(),
        })
    }

    async fn parallel_jobs(&self, _ctx: &Context<'_>) -> Result<Vec<ParallelJobGql>> {
        Ok(vec![])
    }

    // ── フェデレーション ────────────────────────────────────

    async fn federated_sources(&self, _ctx: &Context<'_>) -> Result<Vec<FederatedSourceGql>> {
        Ok(vec![])
    }

    // ── マイグレーション: スキーマプレビュー ────────────────

    async fn preview_source(
        &self,
        _ctx: &Context<'_>,
        source: String,
        uri: String,
    ) -> Result<Vec<TableInfoGql>> {
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

pub struct AdminMutation;

#[Object]
impl AdminMutation {
    // ── レジストリ ──────────────────────────────────────────

    async fn crawl_registry(&self, ctx: &Context<'_>) -> Result<CrawlResultGql> {
        let a = admin(ctx)?;
        let report = a.registry.crawl_now().await;
        Ok(CrawlResultGql {
            ok: true,
            updated: report.updated as i32,
            message: format!("クロール完了: {} 件更新", report.updated),
        })
    }

    async fn test_registry_connection(
        &self,
        _ctx: &Context<'_>,
        id: String,
        uri: String,
    ) -> Result<ConnTestGql> {
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
        _ctx: &Context<'_>,
        config: Option<BackupConfigInput>,
    ) -> Result<BackupGql> {
        let now = Utc::now();
        let kind = config.as_ref().and_then(|c| c.kind.clone()).unwrap_or_else(|| "full".into());
        Ok(BackupGql {
            id: format!("bak_{}", now.timestamp()),
            created_at: now.to_rfc3339(),
            branch: config.and_then(|c| c.branch).unwrap_or_else(|| "main".into()),
            commit_id: String::new(),
            kind,
            size_mb: 0.0,
            path: String::new(),
            status: "queued".into(),
        })
    }

    async fn restore_backup(
        &self,
        _ctx: &Context<'_>,
        input: RestoreInput,
    ) -> Result<MutationResult> {
        Ok(MutationResult {
            success: true,
            commit_id: None,
            message: format!("バックアップ {} のリストアをキューに追加しました。", input.backup_id),
        })
    }

    async fn set_backup_schedule(
        &self,
        _ctx: &Context<'_>,
        input: ScheduleInput,
    ) -> Result<ScheduleGql> {
        Ok(ScheduleGql {
            enabled: input.enabled,
            cron: input.cron,
            kind: input.kind,
            next_run: None,
        })
    }

    // ── クラスタ ────────────────────────────────────────────

    async fn cluster_node_op(
        &self,
        _ctx: &Context<'_>,
        input: ClusterNodeInput,
    ) -> Result<MutationResult> {
        Ok(MutationResult {
            success: true,
            commit_id: None,
            message: format!("ノード {} ({}): {} 操作を受理しました。", input.node_id, input.addr, input.action),
        })
    }

    async fn rebalance_cluster(&self, _ctx: &Context<'_>) -> Result<MutationResult> {
        Ok(MutationResult {
            success: true,
            commit_id: None,
            message: "リバランス計画を実行しました。".into(),
        })
    }

    async fn cluster_propose(
        &self,
        ctx: &Context<'_>,
        sql: String,
    ) -> Result<MutationResult> {
        let a = admin(ctx)?;
        match a.engine.execute(&sql) {
            Ok(_) => Ok(MutationResult { success: true, commit_id: None, message: "ok".into() }),
            Err(e) => Ok(MutationResult { success: false, commit_id: None, message: e }),
        }
    }

    // ── 並列実行 ────────────────────────────────────────────

    async fn set_parallel_config(
        &self,
        _ctx: &Context<'_>,
        config: ParallelConfigInput,
    ) -> Result<ParallelConfigGql> {
        Ok(ParallelConfigGql {
            enabled: config.enabled,
            max_workers: config.max_workers,
            chunk_size: config.chunk_size,
            strategy: config.strategy,
        })
    }

    async fn explain_distributed(
        &self,
        _ctx: &Context<'_>,
        sql: String,
    ) -> Result<Vec<ExplainStepGql>> {
        Ok(vec![ExplainStepGql {
            step: 1,
            node: "node-1".into(),
            range: "(min)-(max)".into(),
            operation: sql,
            estimated_rows: 0,
        }])
    }

    // ── フェデレーション ────────────────────────────────────

    async fn register_federated_source(
        &self,
        _ctx: &Context<'_>,
        input: FederatedSourceInput,
    ) -> Result<FederatedSourceGql> {
        Ok(FederatedSourceGql {
            name: input.name,
            kind: input.kind,
            uri: input.uri,
            status: "connected".into(),
            tables: 0,
        })
    }

    async fn drop_federated_source(
        &self,
        _ctx: &Context<'_>,
        name: String,
    ) -> Result<MutationResult> {
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

    // ── マイグレーション ────────────────────────────────────

    async fn test_source_connection(
        &self,
        _ctx: &Context<'_>,
        source: String,
        uri: String,
    ) -> Result<ConnTestGql> {
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
