//! `--columnar-learner`起動フロー専用の最小限HTTPサーバー(A.6-2実プロセス
//! 検証用)。
//!
//! `ColumnarApplier`(`aruaru-dist::columnar_applier`)を`RaftNode`の
//! `Applier`として注入したlearnerプロセスが、実際にLeaderからの
//! AppendEntriesを受け取って列レプリカを更新できていることを、
//! 別プロセス・実HTTPから観測できるようにするための最小限の
//! observability面。**正直な開示**: 本番の`admin.rs`(`AdminState`+
//! `KeyGuardian`による認証、`/admin/*`共通ミドルウェア)とは独立した、
//! この検証用途に限定した簡易実装——`ARUARU_DB_ADMIN_TOKEN`との
//! 定数時間比較のみを行い、`KeyGuardian`の自動発行キーは受理しない。

use std::sync::Arc;

use poem::http::StatusCode;
use poem::web::{Data, Json, Path};
use poem::{get, handler, listener::TcpListener, EndpointExt, IntoResponse, Request, Response, Route, Server};

use aruaru_backup::table_format::RangeOp;
use aruaru_dist::ColumnarApplier;

fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

fn check_token(req: &Request) -> Result<(), StatusCode> {
    let expected = std::env::var("ARUARU_DB_ADMIN_TOKEN").ok();
    let Some(expected) = expected else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let provided = req
        .headers()
        .get("x-admin-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !provided.is_empty() && constant_time_eq(provided, &expected) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[handler]
async fn healthz() -> &'static str {
    "ok"
}

async fn status_impl(
    table: String,
    q: std::collections::HashMap<String, String>,
    applier: &Arc<ColumnarApplier>,
    req: &Request,
) -> Response {
    if let Err(code) = check_token(req) {
        return code.into_response();
    }
    // A.6-3: ?required_index=N&required_commit_seq=M が付いていれば、
    // Raft index + MVCC(commit 通し番号)で staleness を検証し、
    // 満たさなければ 409 CONFLICT + stale 詳細を返す。
    let required_index = q.get("required_index").and_then(|s| s.parse::<u64>().ok());
    let required_commit_seq = q.get("required_commit_seq").and_then(|s| s.parse::<u64>().ok());
    if required_index.is_some() || required_commit_seq.is_some() {
        match applier.read_at_index(
            &table,
            required_index.unwrap_or(0),
            required_commit_seq.unwrap_or(0),
        ) {
            Ok(view) => {
                return Json(serde_json::json!({
                    "table": table,
                    "stale": false,
                    "appliedIndex": view.applied_index,
                    "appliedCommitSeq": view.applied_commit_seq,
                    "columnarBlockCount": view.blocks.len(),
                    "columnarLiveRowCount": view.live_row_count,
                }))
                .into_response();
            }
            Err(s) => {
                let mut resp = Json(serde_json::json!({
                    "table": table,
                    "stale": true,
                    "appliedIndex": s.applied_index,
                    "requiredIndex": s.required_index,
                    "appliedCommitSeq": s.applied_commit_seq,
                    "requiredCommitSeq": s.required_commit_seq,
                }))
                .into_response();
                resp.set_status(StatusCode::CONFLICT);
                return resp;
            }
        }
    }

    let snapshot_id = applier.latest_snapshot_id(&table);
    let row_count = applier
        .engine()
        .snapshot_table(&table)
        .map(|(_, pks, _)| pks.len())
        .unwrap_or(0);
    // A.6-4 段階2 の観測: base+delta の block 数、MoR 実効行数、
    // deletion vector にマークされた行位置の総数(DELETE/UPDATE が
    // 実際に deletion vector を立てていることを別プロセスから確認できる)。
    let blocks = applier.latest_blocks(&table).unwrap_or_default();
    let block_count = blocks.len();
    let deletion_vector_positions: usize =
        blocks.iter().map(|b| b.deletion_vector.len()).sum();
    Json(serde_json::json!({
        "table": table,
        "snapshotId": snapshot_id,
        "replicationCount": applier.replication_count(),
        "rowCount": row_count,
        "columnarBlockCount": block_count,
        "columnarLiveRowCount": applier.latest_live_row_count(&table),
        "columnarDeletionVectorPositions": deletion_vector_positions,
        "appliedIndex": applier.applied_index(),
        "appliedCommitSeq": applier.applied_commit_seq(),
    }))
    .into_response()
}

/// `Query.htapReplicas` 相当の枝刈り込み観測 API。
/// Connect `Prune`(`{table, column, op: gt|ge|lt|le|eq, value}`)
/// (`op=eq` のときは bloom filter で等値枝刈り、`value` は文字列キー扱い)。
async fn prune_impl(
    table: String,
    params: std::collections::HashMap<String, String>,
    applier: Arc<ColumnarApplier>,
    req: &Request,
) -> Response {
    if let Err(code) = check_token(req) {
        return code.into_response();
    }
    let Some(column) = params.get("column").cloned() else {
        return (StatusCode::BAD_REQUEST, "column is required").into_response();
    };
    let op = params.get("op").map(|s| s.as_str()).unwrap_or("eq");
    let value = params.get("value").cloned().unwrap_or_default();

    let preview = if op == "eq" {
        applier.prune_equality_preview(&table, &column, &value)
    } else {
        let range_op = match op {
            "gt" => RangeOp::Gt,
            "ge" => RangeOp::Ge,
            "lt" => RangeOp::Lt,
            "le" => RangeOp::Le,
            other => {
                return (
                    StatusCode::BAD_REQUEST,
                    format!("unknown op '{other}' (want gt|ge|lt|le|eq)"),
                )
                    .into_response()
            }
        };
        let Ok(v) = value.parse::<f64>() else {
            return (StatusCode::BAD_REQUEST, "value must be a number for range ops").into_response();
        };
        applier.prune_range_preview(&table, &column, range_op, v)
    };

    match preview {
        Some(p) => Json(serde_json::json!({
            "table": table,
            "column": column,
            "op": op,
            "value": value,
            "totalBlocks": p.total_blocks,
            "keptBlocks": p.kept_blocks,
            "skippedBlocks": p.skipped_blocks,
            "keptLiveRows": p.kept_live_rows,
        }))
        .into_response(),
        None => (StatusCode::NOT_FOUND, "table not replicated yet").into_response(),
    }
}

pub const COLUMNAR_SERVICE: &str = "open_runo.aruaru.columnar.v1.Columnar";

fn connect_fail(status: StatusCode, code: &str, message: &str) -> Response {
    let body = serde_json::json!({ "code": code, "message": message }).to_string();
    Response::builder().status(status).content_type("application/json").body(body)
}

/// Connect の単純呼び出し: `POST /open_runo.aruaru.columnar.v1.Columnar/{Status|Prune}`。
/// 本文は JSON オブジェクト(RS-JSON `parse_secure` で上限つき)。`table` は必須、
/// Status は `requiredIndex`・`requiredCommitSeq`、Prune は `column`・`op`・`value` を取る。
#[handler]
async fn connect(
    Path(method): Path<String>,
    body: poem::Body,
    Data(applier): Data<&Arc<ColumnarApplier>>,
    req: &Request,
) -> Response {
    if let Err(code) = check_token(req) {
        return connect_fail(code, "unauthenticated", "invalid or missing x-admin-token header");
    }
    let json_ct = req
        .header("content-type")
        .map(|v| v.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json"))
        .unwrap_or(false);
    if !json_ct {
        return connect_fail(StatusCode::UNSUPPORTED_MEDIA_TYPE, "invalid_argument", "content-type は application/json にしてください");
    }
    let bytes = match body.into_bytes_limit(64 * 1024).await {
        Ok(b) => b,
        Err(_) => return connect_fail(StatusCode::PAYLOAD_TOO_LARGE, "invalid_argument", "本文が大きすぎます"),
    };
    let text = String::from_utf8_lossy(&bytes);
    let limits = rust_json::Limits { max_bytes: 64 * 1024, max_depth: 8, reject_duplicate_keys: true };
    let obj = match rust_json::parse_secure(&text, &limits) {
        Ok(serde_json::Value::Object(o)) => o,
        Ok(_) => return connect_fail(StatusCode::BAD_REQUEST, "invalid_argument", "本文は JSON オブジェクトにしてください"),
        Err(e) => return connect_fail(StatusCode::BAD_REQUEST, "invalid_argument", &format!("invalid JSON: {e}")),
    };
    let get = |k: &str| -> Option<String> {
        obj.get(k).and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
    };
    let Some(table) = get("table") else {
        return connect_fail(StatusCode::BAD_REQUEST, "invalid_argument", "table is required");
    };
    let mut q = std::collections::HashMap::new();
    match method.as_str() {
        "Status" => {
            if let Some(v) = get("requiredIndex") { q.insert("required_index".to_string(), v); }
            if let Some(v) = get("requiredCommitSeq") { q.insert("required_commit_seq".to_string(), v); }
            status_impl(table, q, applier, req).await
        }
        "Prune" => {
            for (k, name) in [("column", "column"), ("op", "op"), ("value", "value")] {
                if let Some(v) = get(k) { q.insert(name.to_string(), v); }
            }
            prune_impl(table, q, applier.clone(), req).await
        }
        _ => connect_fail(StatusCode::NOT_FOUND, "unimplemented", "unknown method"),
    }
}

/// `--columnar-learner`フローから呼ばれる。呼び出し元がRaft driverを
/// 別途`tokio::spawn`していることを前提とし、本関数はHTTPサーバーを
/// 起動して待ち受け続ける(通常のリクエストハンドラループと同様、
/// 戻ってくるのはサーバー停止時のみ)。
pub async fn serve(bind_addr: String, applier: Arc<ColumnarApplier>) -> anyhow::Result<()> {
    let app = Route::new()
        .at("/healthz", get(healthz))
        .at(format!("/{COLUMNAR_SERVICE}/:method"), poem::post(connect))
        .data(applier);
    tracing::info!(bind_addr = %bind_addr, "columnar learner observability server listening");
    Server::new(TcpListener::bind(bind_addr)).run(app).await?;
    Ok(())
}
