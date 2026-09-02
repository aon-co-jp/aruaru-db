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

#[handler]
async fn columnar_status(
    Path(table): Path<String>,
    Data(applier): Data<&Arc<ColumnarApplier>>,
    req: &Request,
) -> Response {
    if let Err(code) = check_token(req) {
        return code.into_response();
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
    }))
    .into_response()
}

/// `Query.htapReplicas` 相当の枝刈り込み観測 API。
/// `GET /columnar/:table/prune?column=<c>&op=<gt|ge|lt|le|eq>&value=<v>`
/// (`op=eq` のときは bloom filter で等値枝刈り、`value` は文字列キー扱い)。
#[handler]
async fn columnar_prune(
    Path(table): Path<String>,
    req: &Request,
) -> Response {
    if let Err(code) = check_token(req) {
        return code.into_response();
    }
    // `.data()` から取り出す(handler 引数の Data<..> は check_token より
    // 後に評価したいので手動で取得)。
    let applier = match req.data::<Arc<ColumnarApplier>>() {
        Some(a) => a.clone(),
        None => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let params: std::collections::HashMap<String, String> = req
        .uri()
        .query()
        .unwrap_or("")
        .split('&')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((
                k.to_string(),
                v.replace('+', " ").replace("%20", " "),
            ))
        })
        .collect();
    let Some(column) = params.get("column").cloned() else {
        return (StatusCode::BAD_REQUEST, "column query param is required").into_response();
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

/// `--columnar-learner`フローから呼ばれる。呼び出し元がRaft driverを
/// 別途`tokio::spawn`していることを前提とし、本関数はHTTPサーバーを
/// 起動して待ち受け続ける(通常のリクエストハンドラループと同様、
/// 戻ってくるのはサーバー停止時のみ)。
pub async fn serve(bind_addr: String, applier: Arc<ColumnarApplier>) -> anyhow::Result<()> {
    let app = Route::new()
        .at("/healthz", get(healthz))
        .at("/columnar/:table", get(columnar_status))
        .at("/columnar/:table/prune", get(columnar_prune))
        .data(applier);
    tracing::info!(bind_addr = %bind_addr, "columnar learner observability server listening");
    Server::new(TcpListener::bind(bind_addr)).run(app).await?;
    Ok(())
}
