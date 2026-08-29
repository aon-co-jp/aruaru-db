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
use async_graphql_poem::{GraphQLBatchRequest, GraphQLBatchResponse};

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

/// 【2026-08-29 再設計 P3】APIキー自己発行の結果。旧 REST
/// `POST /v1/keys/self-issue` の等価。認証ガードは掛けない
/// (「認証不要で即発行できること自体が承認手続き」= SPIFFE 哲学。
/// `docs/CONTROL_PLANE_REDESIGN.md` §2 原則 11)。
#[derive(SimpleObject, Clone)]
pub struct SelfIssuedKeyGql {
    pub key: String,
    pub role: String,
    pub expires_in_hours: i32,
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

    /// 【2026-08-29 再設計 P3】旧 REST `POST /v1/keys/self-issue` の等価。
    /// **認証不要**(このリゾルバだけは admin トークンを要求しない)。
    /// `viewer` ロール・既定 24h TTL の短命キーを即発行する。より強い権限は
    /// `ARUARU_DB_ADMIN_TOKEN` か、管理者が `revokeKeys` で失効させた上で
    /// 別途発行する。`KeyGuardian` が schema data に無い構成では失敗する。
    async fn self_issue_key(&self, ctx: &Context<'_>) -> Result<SelfIssuedKeyGql> {
        let keyring = ctx
            .data::<Arc<aruaru_dist::keyring::KeyGuardian>>()
            .map_err(|_| async_graphql::Error::new("self-issue is not configured on this server"))?;
        let ttl_hours = aruaru_dist::keyring::DEFAULT_SELF_ISSUE_TTL_HOURS;
        let key = keyring.issue("self-issued", "viewer", Some(chrono::Duration::hours(ttl_hours)));
        Ok(SelfIssuedKeyGql { key, role: "viewer".into(), expires_in_hours: ttl_hours as i32 })
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
    // 【2026-08-29 再設計 P3】`selfIssueKey`(認証不要 mutation)は
    // `AdminCtx` ではなく schema data の `KeyGuardian` を直接引く
    // (admin トークンを要求しないため)。`AdminCtx.keyring` と同一インスタンス。
    let keyring = admin_ctx.keyring.clone();
    let mut b = builder().data(engine).data(admin_ctx);
    if let Some(k) = keyring {
        b = b.data(k);
    }
    b.finish()
}

/// Federation SDL を出力 (wgc / hive CLI 用)
pub fn subgraph_sdl() -> String {
    Schema::build(QueryRoot::default(), MutationRoot::default(), EmptySubscription)
        .enable_federation()
        .finish()
        .sdl_with_options(SDLExportOptions::new().federation())
}

/// GraphQLリクエストに同梱された`x-admin-token`ヘッダー値
/// (2026-08-01追加、実バグ修正)。`/graphql`は`QueryRoot`/`MutationRoot`
/// (`VcsQuery`/`VcsMutation`+`AdminQuery`/`AdminMutation`)を1つのスキーマに
/// 統合しているため、エンドポイント全体を一律に認証で塞ぐと通常のVCS
/// クエリ(認証不要であるべき)まで巻き込んでしまう。そのため
/// エンドポイント自体は認証しない代わりに、この値をリクエストデータへ
/// 注入し、`admin_resolvers::require_admin_token`が`AdminQuery`/
/// `AdminMutation`配下の各フィールド解決時に個別に検証する。
#[derive(Clone, Default)]
pub(crate) struct GraphqlAdminToken(pub Option<String>);

/// Poem エンドポイント。
///
/// **2026-08-01追記(実バグ修正)**: 従来は`GraphQL::new(schema)`
/// (`async-graphql-poem`の既定エンドポイント)をそのまま使っており、
/// `x-admin-token`ヘッダーがGraphQL実行コンテキストへ一切伝播していな
/// かった——`admin.rs`(REST)側は2026-07-30に`/admin/*`全体へ認証を
/// 遡及適用済みだったが、**同じ管理操作をGraphQL経由(`cluster_propose`・
/// `create_backup`・`run_migration`等)で呼べば無認証のまま実行できて
/// しまう抜け穴**が残っていた(ユーザー指示「aruaru-serverは外部から
/// 乗っ取られないようにセキュリティをしっかりして」の趣旨に反する)。
/// ヘッダーを読み取り`GraphqlAdminToken`としてリクエストデータへ注入する
/// 薄いハンドラへ置き換え、実際の検証は各Admin resolverで行う
/// (`admin_resolvers::require_admin_token`参照)。
#[poem::handler]
async fn graphql_handler(
    req: &poem::Request,
    gql_req: GraphQLBatchRequest,
    schema: poem::web::Data<&AruaruSchema>,
) -> GraphQLBatchResponse {
    let token = req.header("x-admin-token").map(|v| v.to_string());
    let batch_req = gql_req.0.data(GraphqlAdminToken(token));
    schema.execute_batch(batch_req).await.into()
}

pub fn graphql_endpoint(engine: Arc<QueryEngine>, admin_ctx: AdminCtx) -> impl poem::Endpoint {
    use poem::EndpointExt;
    graphql_handler.data(build_schema(engine, admin_ctx))
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
