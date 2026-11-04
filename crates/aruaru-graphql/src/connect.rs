//! 管理操作の Connect(単純呼び出し)層(2026-11-04、REST 全廃方針)。
//!
//! `POST /open_runo.aruaru.admin.v1.Admin/<Method>`、`content-type: application/json`、
//! 本文は JSON オブジェクト。本文のキーは、対応する管理操作の引数名(camelCase)そのもの。
//! 実体は既存の管理リゾルバ(`admin_resolvers`)で、認証(`x-admin-token`)も同じ検証を通る。
//! 失敗は `{"code","message"}`(open-easy-web の connect.rs と同じ形)。
//! JSON は RS-JSON の `parse_secure`(サイズ・深さ・重複キーの上限つき)で読む。

use crate::{AruaruSchema, GraphqlAdminToken};
use poem::{handler, http::StatusCode, web::Data, Request, Response};

pub const SERVICE: &str = "open_runo.aruaru.admin.v1.Admin";

struct Op {
    method: &'static str,
    mutation: bool,
    /// GraphQL 変数宣言(`$x: T!, ...`)
    vars: &'static str,
    /// フィールド呼び出し(引数つき)と選択セット
    field: &'static str,
}

const fn op(method: &'static str, mutation: bool, vars: &'static str, field: &'static str) -> Op {
    Op { method, mutation, vars, field }
}

const OPS: &[Op] = &[
    op("Registry", false, "", "registry { id name category wire status rank score updatedAt }"),
    op("RegistrySummary", false, "", "registrySummary { total connectable ga beta pgCompatible planned }"),
    op("ListBackups", false, "", "backups { id createdAt branch commitId kind sizeMb path status }"),
    op("GetBackupSchedule", false, "", "backupSchedule { enabled cron kind nextRun }"),
    op(
        "ClusterStatus",
        false,
        "",
        "clusterStatus { stats { totalNodes healthyNodes totalRanges totalRows tableCount replicationFactor underReplicated } nodes { nodeId addr role alive commitIndex appliedIndex ranges diskUsedGb } ranges { rangeId startKey endKey leaderNode replicas sizeMb } }",
    ),
    op("ListFederatedSources", false, "", "federatedSources { name kind uri status tables }"),
    op(
        "PreviewSource",
        false,
        "$source: String!, $uri: String!",
        "previewSource(source: $source, uri: $uri) { schema name estimatedRows }",
    ),
    op(
        "ExplainDistributed",
        false,
        "$sql: String!",
        "explainDistributed(sql: $sql) { step node range operation estimatedRows }",
    ),
    op(
        "CreateBackup",
        true,
        "$config: BackupConfigInput",
        "createBackup(config: $config) { id createdAt branch commitId kind sizeMb path status }",
    ),
    op("RestoreBackup", true, "$input: RestoreInput!", "restoreBackup(input: $input) { success commitId message }"),
    op(
        "SetBackupSchedule",
        true,
        "$input: ScheduleInput!",
        "setBackupSchedule(input: $input) { enabled cron kind nextRun }",
    ),
    op("ClusterNodeOp", true, "$input: ClusterNodeInput!", "clusterNodeOp(input: $input) { success commitId message }"),
    op("RebalanceCluster", true, "", "rebalanceCluster { success commitId message }"),
    op("ClusterPropose", true, "$sql: String!", "clusterPropose(sql: $sql) { success commitId message }"),
    op(
        "RegisterFederatedSource",
        true,
        "$input: FederatedSourceInput!",
        "registerFederatedSource(input: $input) { name kind uri status tables }",
    ),
    op("DropFederatedSource", true, "$name: String!", "dropFederatedSource(name: $name) { success commitId message }"),
    op("FederatedQuery", true, "$sql: String!", "federatedQuery(sql: $sql) { columns rows commandTag }"),
    op(
        "TestSourceConnection",
        true,
        "$source: String!, $uri: String!",
        "testSourceConnection(source: $source, uri: $uri) { ok message serverVersion }",
    ),
    op(
        "RunMigration",
        true,
        "$input: MigrateInput!",
        "runMigration(input: $input) { success wire totalRows commitId message tables { table rows error } }",
    ),
    op("CrawlRegistry", true, "", "crawlRegistry { ok updated message }"),
    op(
        "TestRegistryConnection",
        true,
        "$id: String!, $uri: String!",
        "testRegistryConnection(id: $id, uri: $uri) { ok message serverVersion }",
    ),
];

fn fail(status: StatusCode, code: &str, message: &str) -> Response {
    let body = serde_json::json!({ "code": code, "message": message }).to_string();
    Response::builder().status(status).content_type("application/json").body(body)
}

fn parse_object(text: &str) -> Result<serde_json::Value, String> {
    if text.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    let limits = rust_json::Limits { max_bytes: 1024 * 1024, max_depth: 16, reject_duplicate_keys: true };
    match rust_json::parse_secure(text, &limits) {
        Ok(v @ serde_json::Value::Object(_)) => Ok(v),
        Ok(_) => Err("本文は JSON オブジェクトにしてください".into()),
        Err(e) => Err(format!("invalid JSON: {e}")),
    }
}

#[handler]
pub async fn connect_handler(
    req: &Request,
    poem::web::Path(method): poem::web::Path<String>,
    body: poem::Body,
    schema: Data<&AruaruSchema>,
) -> Response {
    let json_ct = req
        .header("content-type")
        .map(|v| v.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json"))
        .unwrap_or(false);
    let token = req.header("x-admin-token").map(|v| v.to_string());
    if token.as_deref().unwrap_or("").is_empty() {
        return fail(StatusCode::UNAUTHORIZED, "unauthenticated", "invalid or missing x-admin-token header");
    }
    let Some(op) = OPS.iter().find(|o| o.method == method) else {
        return fail(StatusCode::NOT_FOUND, "unimplemented", "unknown method");
    };
    if !json_ct {
        return fail(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "invalid_argument",
            "content-type は application/json にしてください",
        );
    }
    let bytes = match body.into_bytes_limit(1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::PAYLOAD_TOO_LARGE, "invalid_argument", "本文が大きすぎます"),
    };
    let text = match std::str::from_utf8(&bytes) {
        Ok(t) => t,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "invalid_argument", "本文が UTF-8 ではありません"),
    };
    let vars = match parse_object(text) {
        Ok(v) => v,
        Err(e) => return fail(StatusCode::BAD_REQUEST, "invalid_argument", &e),
    };
    let decl = if op.vars.is_empty() { String::new() } else { format!("({})", op.vars) };
    let kw = if op.mutation { "mutation" } else { "query" };
    let doc = format!("{kw} Op{decl} {{ {} }}", op.field);
    let gql = async_graphql::Request::new(doc)
        .variables(async_graphql::Variables::from_json(vars))
        .data(GraphqlAdminToken(token));
    let resp = schema.execute(gql).await;
    if let Some(err) = resp.errors.first() {
        let m = err.message.clone();
        let (st, code) = if m.contains("x-admin-token") || m.contains("not configured") {
            (StatusCode::UNAUTHORIZED, "unauthenticated")
        } else if err.path.is_empty() || m.starts_with("Variable") || m.starts_with("Invalid value") {
            (StatusCode::BAD_REQUEST, "invalid_argument")
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, "internal")
        };
        return fail(st, code, &m);
    }
    let data = resp.data.into_json().unwrap_or(serde_json::Value::Null);
    // 戻り値は選択した 1 フィールドの中身
    let out = data.as_object().and_then(|o| o.values().next().cloned()).unwrap_or(serde_json::Value::Null);
    Response::builder().content_type("application/json").body(out.to_string())
}

/// `POST /open_runo.aruaru.admin.v1.Admin/:method`
pub fn connect_endpoint(schema: AruaruSchema) -> impl poem::Endpoint {
    use poem::{post, EndpointExt, Route};
    Route::new().at(format!("/{SERVICE}/:method"), post(connect_handler)).data(schema)
}
