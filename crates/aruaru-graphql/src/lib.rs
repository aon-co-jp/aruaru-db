//! aruaru-graphql: WunderGraph Cosmo / Hive Gateway 対応 **Federation サブグラフ**
//!
//! REST を廃止し、GraphQL 一本で全管理操作を提供する。
//! 将来 Hive Gateway（MIT）を差し込む際は VITE_ARUARU_GQL_ENDPOINT を
//! ゲートウェイ URL に切り替えるだけで、このサブグラフは変更不要。
//!
//! ## スキーマ構成
//! - **バージョン管理系** (QueryRoot / MutationRoot): コミット・ブランチ・SQL・diff
//! - **管理系** (AdminQuery / AdminMutation): レジストリ・バックアップ・クラスタ・
//!   マイグレーション・並列・フェデレーション

pub mod admin_resolvers;
pub mod admin_types;

use std::sync::Arc;

use async_graphql::{
    Context, EmptySubscription, MergedObject, Object, Result, Schema, SchemaBuilder,
    SimpleObject, SDLExportOptions, ID,
};
use async_graphql_poem::GraphQL;

use aruaru_query::{QueryEngine, QueryResponse};

pub use admin_resolvers::{AdminCtx, AdminMutation, AdminQuery};

// ── データ型 ──────────────────────────────────────────────────

/// Federation エンティティ: Commit (@key: id)
#[derive(SimpleObject, Clone)]
pub struct CommitGql {
    pub id: ID,
    pub short_id: String,
    pub author: String,
    pub message: String,
    pub timestamp: String,
    pub root_hash: String,
}

#[derive(SimpleObject, Clone)]
pub struct BranchGql {
    pub name: String,
    pub head_commit_id: ID,
    pub is_current: bool,
}

#[derive(SimpleObject, Clone)]
pub struct DiffGql {
    pub from_commit: String,
    pub to_commit: String,
    pub added: i32,
    pub removed: i32,
    pub modified: i32,
}

#[derive(SimpleObject, Clone)]
pub struct QueryResultGql {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub command_tag: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct MutationResult {
    pub success: bool,
    pub commit_id: Option<String>,
    pub message: String,
}

fn engine<'a>(ctx: &Context<'a>) -> Result<&'a Arc<QueryEngine>> {
    ctx.data::<Arc<QueryEngine>>()
        .map_err(|_| async_graphql::Error::new("QueryEngine not in context"))
}

// ── バージョン管理 Query ──────────────────────────────────────

#[derive(Default)]
pub struct VcsQuery;

#[Object]
impl VcsQuery {
    async fn current_branch(&self, ctx: &Context<'_>) -> Result<String> {
        Ok(engine(ctx)?.version().current_branch())
    }

    async fn branches(&self, ctx: &Context<'_>) -> Result<Vec<BranchGql>> {
        Ok(engine(ctx)?.version().list_branches().into_iter().map(|b| BranchGql {
            name: b.name,
            head_commit_id: ID(b.head.as_str().to_string()),
            is_current: b.is_current,
        }).collect())
    }

    async fn log(&self, ctx: &Context<'_>, #[graphql(default = 20)] limit: i32) -> Result<Vec<CommitGql>> {
        Ok(engine(ctx)?.version().log(limit.max(0) as usize).into_iter().map(commit_to_gql).collect())
    }

    async fn diff(&self, ctx: &Context<'_>, from: String, to: String) -> Result<DiffGql> {
        let eng = engine(ctx)?;
        match eng.version().diff_branches(eng.store(), &from, &to) {
            Ok(d) => Ok(DiffGql {
                from_commit: from, to_commit: to,
                added: d.added_count() as i32,
                removed: d.removed_count() as i32,
                modified: d.modified_count() as i32,
            }),
            Err(e) => Err(async_graphql::Error::new(e.to_string())),
        }
    }

    async fn sql(&self, ctx: &Context<'_>, query: String) -> Result<QueryResultGql> {
        let resp = engine(ctx)?.execute_async(&query).await.map_err(async_graphql::Error::new)?;
        Ok(response_to_gql(resp))
    }

    #[graphql(entity)]
    async fn find_commit_by_id(&self, ctx: &Context<'_>, id: ID) -> Result<CommitGql> {
        let target = id.to_string();
        engine(ctx)?.version().log(100_000).into_iter()
            .find(|c| c.id.as_str() == target || c.id.short() == target)
            .map(commit_to_gql)
            .ok_or_else(|| async_graphql::Error::new(format!("commit not found: {target}")))
    }
}

// ── バージョン管理 Mutation ───────────────────────────────────

#[derive(Default)]
pub struct VcsMutation;

#[Object]
impl VcsMutation {
    async fn create_branch(&self, ctx: &Context<'_>, name: String) -> Result<MutationResult> {
        match engine(ctx)?.version().create_branch(&name) {
            Ok(_) => Ok(MutationResult { success: true, commit_id: None, message: format!("branch '{name}' created") }),
            Err(e) => Ok(MutationResult { success: false, commit_id: None, message: e.to_string() }),
        }
    }

    async fn checkout(&self, ctx: &Context<'_>, branch: String) -> Result<MutationResult> {
        match engine(ctx)?.version().checkout(&branch) {
            Ok(_) => Ok(MutationResult { success: true, commit_id: None, message: format!("switched to '{branch}'") }),
            Err(e) => Ok(MutationResult { success: false, commit_id: None, message: e.to_string() }),
        }
    }

    async fn merge(&self, ctx: &Context<'_>, from_branch: String) -> Result<MutationResult> {
        match engine(ctx)?.version().fast_forward_merge(&from_branch) {
            Ok(id) => Ok(MutationResult { success: true, commit_id: Some(id.short().to_string()), message: format!("merged '{from_branch}'") }),
            Err(e) => Ok(MutationResult { success: false, commit_id: None, message: e.to_string() }),
        }
    }

    async fn exec_sql(
        &self, ctx: &Context<'_>,
        sql: String,
        #[graphql(default = false)] auto_commit: bool,
        commit_message: Option<String>,
    ) -> Result<MutationResult> {
        let eng = engine(ctx)?;
        if let Err(e) = eng.execute_async(&sql).await {
            return Ok(MutationResult { success: false, commit_id: None, message: e });
        }
        let mut commit_id = None;
        if auto_commit {
            let msg = commit_message.unwrap_or_else(|| "exec_sql".into()).replace('\'', "''");
            if let Ok(QueryResponse::Rows { rows, .. }) = eng.execute(&format!("SELECT aruaru_commit('{msg}')")) {
                commit_id = rows.first().and_then(|r| r.first()).map(|v| v.as_text());
            }
        }
        Ok(MutationResult { success: true, commit_id, message: "ok".into() })
    }
}

// ── 統合 Query / Mutation (MergedObject) ─────────────────────

/// VCS + Admin を1つの Query に束ねる
#[derive(MergedObject, Default)]
pub struct QueryRoot(VcsQuery, AdminQuery);

/// VCS + Admin を1つの Mutation に束ねる
#[derive(MergedObject, Default)]
pub struct MutationRoot(VcsMutation, AdminMutation);

// ── スキーマ構築 ───────────────────────────────────────────────

pub type AruaruSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

fn builder() -> SchemaBuilder<QueryRoot, MutationRoot, EmptySubscription> {
    Schema::build(QueryRoot::default(), MutationRoot::default(), EmptySubscription)
        .enable_federation()
}

/// エンジンと管理状態を注入してスキーマを構築
pub fn build_schema(engine: Arc<QueryEngine>, admin_ctx: AdminCtx) -> AruaruSchema {
    builder().data(engine).data(admin_ctx).finish()
}

/// Federation SDL を出力 (wgc / hive CLI 用)
pub fn subgraph_sdl() -> String {
    Schema::build(QueryRoot::default(), MutationRoot::default(), EmptySubscription)
        .enable_federation()
        .finish()
        .sdl_with_options(SDLExportOptions::new().federation())
}

/// Poem エンドポイント
pub fn graphql_endpoint(engine: Arc<QueryEngine>, admin_ctx: AdminCtx) -> impl poem::Endpoint {
    GraphQL::new(build_schema(engine, admin_ctx))
}

// ── 変換ヘルパ ────────────────────────────────────────────────

fn commit_to_gql(c: aruaru_core::Commit) -> CommitGql {
    let timestamp = c.timestamp_rfc3339();
    CommitGql {
        id: ID(c.id.as_str().to_string()),
        short_id: c.id.short().to_string(),
        author: c.author,
        message: c.message,
        timestamp,
        root_hash: hex::encode(c.root_hash),
    }
}

pub(crate) fn response_to_gql(resp: QueryResponse) -> QueryResultGql {
    match resp {
        QueryResponse::Rows { columns, rows } => QueryResultGql {
            columns,
            rows: rows.into_iter().map(|r| r.iter().map(|v| v.as_text()).collect()).collect(),
            command_tag: None,
        },
        QueryResponse::Command { tag } => QueryResultGql {
            columns: vec![], rows: vec![], command_tag: Some(tag),
        },
    }
}
