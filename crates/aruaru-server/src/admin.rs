//! 管理 REST API (`/admin/*`)
//!
//! Tauri Admin GUI (admin/) から呼ばれる管理エンドポイント。
//! GraphQL と同じ Poem サーバにマウントする。
//!
//! ## 実装方針 (v0.3 段階)
//! - **実データで返せるもの**: クラスタ状態(単一ノード)、バックアップ台帳、
//!   並列設定、分散実行プラン、ローカル SQL のフェデレーテッドクエリ、接続テスト
//! - **エンジン未完なもの**: 実バックアップ I/O、外部DBからの取り込み、
//!   リモートプッシュダウン → 受理してジョブIDを返し、正直に「未実装」を message に記す
//!   (aruaru-backup / aruaru-migrate / 分散実行が完成したら差し替え)

use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use poem::{get, handler, post, web::Data, web::Json, EndpointExt, Route};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use aruaru_query::parser::{self, Statement};
use aruaru_query::{QueryEngine, QueryResponse, Value as SqlValue};
use aruaru_registry::Registry;

use crate::cluster::ClusterNode;

// ── 共有状態 ───────────────────────────────────────────────────

pub struct AdminState {
    pub engine: Arc<QueryEngine>,
    pub registry: Arc<Registry>,
    backups: Mutex<Vec<BackupManifest>>,
    schedule: Mutex<Option<ScheduleInfo>>,
    parallel: Mutex<ParallelConfig>,
    federation: Mutex<Vec<FederatedSource>>,
    /// クラスタトポロジ (Range 配置 + ノード)
    topology: Mutex<aruaru_dist::ClusterTopology>,
    /// Raft ノード (クラスタモード時のみ Some)
    cluster: Mutex<Option<Arc<ClusterNode>>>,
}

impl AdminState {
    pub fn new(engine: Arc<QueryEngine>, registry: Arc<Registry>) -> Arc<Self> {
        Arc::new(Self {
            engine,
            registry,
            backups: Mutex::new(Vec::new()),
            schedule: Mutex::new(None),
            parallel: Mutex::new(ParallelConfig::default()),
            federation: Mutex::new(Vec::new()),
            topology: Mutex::new(aruaru_dist::ClusterTopology::single_node(1, "127.0.0.1:5432")),
            cluster: Mutex::new(None),
        })
    }

    /// Raft ノードを取り付ける (クラスタモード起動時)
    pub fn attach_cluster(&self, node: Arc<ClusterNode>) {
        *self.cluster.lock() = Some(node);
    }

    pub fn cluster_node(&self) -> Option<Arc<ClusterNode>> {
        self.cluster.lock().clone()
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

// ── 型定義 ─────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
struct BackupManifest {
    id: String,
    kind: String,
    started_at: String,
    finished_at: String,
    size_bytes: u64,
    row_count: u64,
    commit_id: String,
    branch: String,
}

#[derive(Clone, Serialize)]
struct ScheduleInfo {
    cron: String,
    enabled: bool,
    kind: String,
    updated_at: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct ParallelConfig {
    max_parallelism: u32,
    worker_threads_per_node: u32,
    enable_parallel_scan: bool,
    enable_parallel_aggregate: bool,
    enable_shuffle_join: bool,
    shuffle_partitions: u32,
    broadcast_threshold_mb: u32,
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            max_parallelism: 8,
            worker_threads_per_node: 4,
            enable_parallel_scan: true,
            enable_parallel_aggregate: true,
            enable_shuffle_join: true,
            shuffle_partitions: 64,
            broadcast_threshold_mb: 32,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct FederatedSource {
    name: String,
    kind: String,
    uri: String,
    read_only: bool,
    pushdown: bool,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    table_count: Option<u32>,
}

// ── リクエスト型 ───────────────────────────────────────────────

#[derive(Deserialize)]
struct BackupRequest {
    kind: String,
    dest_type: String,
    dest_uri: String,
    encrypt: bool,
    retention_days: u32,
    branch: String,
}

#[derive(Deserialize)]
struct RestoreRequest {
    backup_id: String,
    target_branch: String,
    point_in_time: Option<String>,
}

#[derive(Deserialize)]
struct ScheduleRequest {
    cron: String,
    enabled: bool,
    kind: String,
}

#[derive(Deserialize)]
struct SourceUriRequest {
    #[serde(default)]
    source: String,
    #[serde(default)]
    kind: String,
    uri: String,
}

#[derive(Deserialize)]
struct SqlRequest {
    sql: String,
}

#[derive(Deserialize)]
struct DropRequest {
    name: String,
}

#[derive(Deserialize)]
struct NodeRequest {
    action: String,
    node_id: u64,
    #[serde(default)]
    addr: String,
}

#[derive(Deserialize)]
struct RegistryTestRequest {
    id: String,
    uri: String,
}

// ═══════════════════════════════════════════════════════════════
// ルーティング
// ═══════════════════════════════════════════════════════════════

pub fn admin_routes(state: Arc<AdminState>) -> impl poem::Endpoint {
    Route::new()
        .at("/backup", get(list_backups).post(create_backup))
        .at("/backup/restore", post(restore_backup))
        .at("/backup/schedule", post(set_schedule))
        .at("/migrate/test", post(migrate_test))
        .at("/migrate/preview", post(migrate_preview))
        .at("/migrate/run", post(migrate_run))
        .at("/migrate/instance", post(migrate_instance))
        .at("/parallel", get(get_parallel).post(set_parallel))
        .at("/parallel/explain", post(explain_distributed))
        .at("/parallel/jobs", get(list_jobs))
        .at("/federation", get(list_federation).post(register_federation))
        .at("/federation/test", post(federation_test))
        .at("/federation/drop", post(drop_federation))
        .at("/federation/query", post(federated_query))
        .at("/cluster", get(cluster_status))
        .at("/cluster/node", post(cluster_node))
        .at("/cluster/rebalance", post(cluster_rebalance))
        .at("/cluster/propose", post(cluster_propose))
        .at("/registry", get(registry_list))
        .at("/registry/summary", get(registry_summary))
        .at("/registry/crawl", post(registry_crawl))
        .at("/registry/test", post(registry_test_connection))
        // Raft ノード間 RPC 受信エンドポイント
        .at("/raft/append", post(raft_append))
        .at("/raft/vote", post(raft_vote))
        .data(state)
}

// ── ① バックアップ ─────────────────────────────────────────────

#[handler]
fn list_backups(state: Data<&Arc<AdminState>>) -> Json<Value> {
    let backups = state.backups.lock().clone();
    Json(json!({ "backups": backups }))
}

#[handler]
fn create_backup(state: Data<&Arc<AdminState>>, Json(req): Json<BackupRequest>) -> Json<Value> {
    // 現在の HEAD コミットと行数から台帳エントリを作る。
    // 実体の書き出し(圧縮/暗号化/転送)は aruaru-backup 完成後に接続。
    let commits = state.engine.version().log(1);
    let commit_id = commits
        .first()
        .map(|c| c.id.short().to_string())
        .unwrap_or_else(|| "genesis".to_string());
    let rows = state.engine.total_rows() as u64;

    let manifest = BackupManifest {
        id: format!("bkp_{}", chrono::Utc::now().timestamp_millis()),
        kind: req.kind,
        started_at: now(),
        finished_at: now(),
        size_bytes: rows * 64, // 概算
        row_count: rows,
        commit_id,
        branch: req.branch,
    };
    state.backups.lock().push(manifest.clone());

    Json(json!({
        "success": true,
        "manifest": manifest,
        "dest": format!("{}:{}", req.dest_type, req.dest_uri),
        "encrypted": req.encrypt,
        "retention_days": req.retention_days,
        "note": "台帳に記録しました。実体書き出しは aruaru-backup 実装後に有効化されます。"
    }))
}

#[handler]
fn restore_backup(state: Data<&Arc<AdminState>>, Json(req): Json<RestoreRequest>) -> Json<Value> {
    let exists = state.backups.lock().iter().any(|b| b.id == req.backup_id);
    if !exists {
        return Json(json!({ "success": false, "message": format!("バックアップが見つかりません: {}", req.backup_id) }));
    }
    Json(json!({
        "success": true,
        "message": format!(
            "リストアを受理 (backup={}, branch={}{})",
            req.backup_id,
            req.target_branch,
            req.point_in_time.map(|t| format!(", PITR={t}")).unwrap_or_default()
        ),
        "note": "実リストアは aruaru-backup 実装後に有効化されます。"
    }))
}

#[handler]
fn set_schedule(state: Data<&Arc<AdminState>>, Json(req): Json<ScheduleRequest>) -> Json<Value> {
    *state.schedule.lock() = Some(ScheduleInfo {
        cron: req.cron.clone(),
        enabled: req.enabled,
        kind: req.kind,
        updated_at: now(),
    });
    Json(json!({ "success": true, "message": format!("スケジュール更新: {} (enabled={})", req.cron, req.enabled) }))
}

// ── ② お引越し ─────────────────────────────────────────────────

/// host:port を URI から雑に抽出して TCP 到達性を確認する
fn tcp_reachable(uri: &str) -> Result<String, String> {
    // scheme://[user:pass@]host:port/...
    let after_scheme = uri.splitn(2, "://").nth(1).unwrap_or(uri);
    let authority = after_scheme.split('/').next().unwrap_or("");
    let hostport = authority.rsplit('@').next().unwrap_or(authority);
    if hostport.is_empty() {
        return Err("host:port を解析できません".into());
    }
    // ToSocketAddrs で名前解決 (到達確認は呼び出し側で軽く)
    match hostport.to_socket_addrs() {
        Ok(mut addrs) => match addrs.next() {
            Some(a) => Ok(a.to_string()),
            None => Err(format!("解決できませんでした: {hostport}")),
        },
        Err(e) => Err(format!("{hostport}: {e}")),
    }
}

#[handler]
fn migrate_test(Json(req): Json<SourceUriRequest>) -> Json<Value> {
    let src = if req.source.is_empty() { &req.kind } else { &req.source };
    match src.as_str() {
        "csv" | "parquet" => {
            let ok = std::path::Path::new(&req.uri).exists();
            Json(json!({ "ok": ok, "message": if ok { "ファイルが存在します".to_string() } else { "ファイルが見つかりません".to_string() } }))
        }
        _ => match tcp_reachable(&req.uri) {
            Ok(addr) => Json(json!({ "ok": true, "message": format!("解決OK: {addr}") })),
            Err(e) => Json(json!({ "ok": false, "message": e })),
        },
    }
}

#[handler]
fn migrate_preview(Json(req): Json<SourceUriRequest>) -> Json<Value> {
    // 外部ドライバ未接続のため、スキーマの実取得は未実装。
    Json(json!({
        "ok": true,
        "tables": [],
        "note": format!("'{}' のスキーマ取得は外部コネクタ実装後 (v0.6) に有効化されます。", if req.source.is_empty() { &req.kind } else { &req.source })
    }))
}

/// source 文字列 → 取り込みワイヤ
fn wire_for_source(source: &str) -> Option<aruaru_registry::Wire> {
    use aruaru_registry::Wire;
    match source.to_lowercase().as_str() {
        "postgres" | "postgresql" | "cockroach" | "cockroachdb" | "yugabyte" | "yugabytedb"
        | "redshift" | "alloydb" | "greenplum" | "materialize" | "citus" | "risingwave"
        | "questdb" | "cratedb" | "supabase" | "neon" | "timescaledb" | "aruaru" => {
            Some(Wire::Postgres)
        }
        "mysql" | "mariadb" | "tidb" | "singlestore" | "starrocks" | "doris" | "vitess"
        | "oceanbase" | "polardb" | "percona" => Some(Wire::MySQL),
        _ => None,
    }
}

#[handler]
async fn migrate_run(state: Data<&Arc<AdminState>>, Json(req): Json<Value>) -> Json<Value> {
    let source = req.get("source").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let uri = req.get("source_uri").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let commit_message = req
        .get("commit_message")
        .and_then(|v| v.as_str())
        .unwrap_or("Migration import")
        .to_string();
    let include: Vec<String> = req
        .get("include_tables")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();

    // ファイル系 (csv/parquet) は別経路。ここではワイヤ取り込みを実行。
    let Some(wire) = wire_for_source(&source) else {
        return Json(json!({
            "accepted": true,
            "job_id": format!("mig_{}", chrono::Utc::now().timestamp_millis()),
            "message": format!("'{source}' はファイル取り込み系のため、別経路 (CSV/Parquet ローダ) を使用してください。"),
            "note": "ワイヤ取り込み (PostgreSQL/MySQL 互換) のみ即時実行に対応しています。"
        }));
    };

    let Some(adapter) = aruaru_registry::adapter::adapter_for(wire) else {
        return Json(json!({ "accepted": false, "message": format!("{:?} 用アダプタは未実装です。", wire) }));
    };

    // 取り込み元のテーブル一覧
    let tables = match adapter.list_tables(&uri).await {
        Ok(t) => t,
        Err(e) => return Json(json!({ "accepted": false, "message": format!("テーブル一覧取得に失敗: {e}") })),
    };

    let mut imported = Vec::new();
    let mut total_rows = 0usize;
    for t in &tables {
        if !include.is_empty() && !include.contains(&t.name) {
            continue;
        }
        match adapter.read_table(&uri, &t.schema, &t.name, 100_000).await {
            Ok((columns, rows)) => {
                let n = state.engine.ingest_table(&t.name, columns, rows);
                total_rows += n;
                imported.push(json!({ "table": t.name, "rows": n }));
            }
            Err(e) => {
                imported.push(json!({ "table": t.name, "error": e.to_string() }));
            }
        }
    }

    // 取り込み後にコミット (永続ストア設定時は自動 persist)
    let commit = state
        .engine
        .execute(&format!("SELECT aruaru_commit('{}')", commit_message.replace('\'', "''")));
    let commit_id = match commit {
        Ok(QueryResponse::Rows { rows, .. }) => rows
            .first()
            .and_then(|r| r.first())
            .map(|v| v.as_text())
            .unwrap_or_default(),
        _ => String::new(),
    };

    Json(json!({
        "accepted": true,
        "wire": adapter.wire_name(),
        "imported": imported,
        "total_rows": total_rows,
        "commit_id": commit_id,
        "message": format!("{} 経由で {} テーブル / {} 行を取り込みコミットしました。", adapter.wire_name(), imported.len(), total_rows),
    }))
}

#[handler]
fn migrate_instance(Json(req): Json<Value>) -> Json<Value> {
    let target = req.get("target_uri").and_then(|v| v.as_str()).unwrap_or("?");
    let with_history = req.get("include_history").and_then(|v| v.as_bool()).unwrap_or(true);
    Json(json!({
        "accepted": true,
        "job_id": format!("relo_{}", chrono::Utc::now().timestamp_millis()),
        "message": format!("移植ジョブを受理 (target={target}, history={with_history})"),
        "note": "Prolly Tree 共有チャンク転送による移植は分散レイヤ実装後に有効化されます。"
    }))
}

// ── ③ 分散並列化 ───────────────────────────────────────────────

#[handler]
fn get_parallel(state: Data<&Arc<AdminState>>) -> Json<Value> {
    Json(serde_json::to_value(state.parallel.lock().clone()).unwrap())
}

#[handler]
fn set_parallel(state: Data<&Arc<AdminState>>, Json(cfg): Json<ParallelConfig>) -> Json<Value> {
    *state.parallel.lock() = cfg;
    Json(json!({ "success": true }))
}

/// SQL から分散実行プラン (フラグメント列) を生成する。
/// 集計を含めば ParallelScan→Shuffle→HashAggregate→Gather、
/// 単純検索なら ParallelScan→Gather。並列度は設定値・行数から決める。
#[handler]
fn explain_distributed(state: Data<&Arc<AdminState>>, Json(req): Json<SqlRequest>) -> Json<Value> {
    let cfg = state.parallel.lock().clone();
    let kind = aruaru_query::classify_query(&req.sql);

    // 対象テーブルと行数を推定
    let table = match parser::parse(&req.sql) {
        Ok(Statement::Select { table, .. }) => Some(table),
        _ => None,
    };
    let rows = table
        .as_ref()
        .and_then(|t| state.engine.table_row_count(t))
        .unwrap_or(0) as u64;
    // 単一ノード。クラスタ化後に複数ノードへ分散する。
    let scan_par = cfg.max_parallelism.min(8);

    let mut frag_id = 0u32;
    let mut next = || {
        frag_id += 1;
        frag_id
    };

    let mut fragments = vec![json!({
        "id": next(),
        "op": "ParallelScan",
        "parallelism": scan_par,
        "node_ids": [1],
        "est_rows": rows,
        "detail": format!("{} を Range 並列スキャン{}", table.clone().unwrap_or_else(|| "(table)".into()),
            if cfg.enable_parallel_scan { " (述語プッシュダウン)" } else { " (並列スキャン無効)" }),
    })];

    if matches!(kind, aruaru_query::QueryKind::Olap) {
        fragments.push(json!({
            "id": next(), "op": "ShuffleExchange", "parallelism": cfg.shuffle_partitions,
            "node_ids": [1], "est_rows": rows,
            "detail": format!("ハッシュ再分配 ({} パーティション)", cfg.shuffle_partitions),
        }));
        fragments.push(json!({
            "id": next(), "op": "HashAggregate",
            "parallelism": if cfg.enable_parallel_aggregate { scan_par } else { 1 },
            "node_ids": [1], "est_rows": rows / 10 + 1,
            "detail": "部分集計 → マージ (2段階集計)",
        }));
    }

    fragments.push(json!({
        "id": next(), "op": "Gather (Coordinator)", "parallelism": 1,
        "node_ids": [1], "est_rows": rows / 10 + 1,
        "detail": "全フラグメントの結果を集約",
    }));

    // プラン表示は「下から上」なので逆順に
    fragments.reverse();
    Json(json!({ "fragments": fragments, "query_kind": format!("{:?}", kind) }))
}

#[handler]
fn list_jobs(_state: Data<&Arc<AdminState>>) -> Json<Value> {
    // 組み込み単一ノードでは長時間ジョブの常駐管理は未実装。
    Json(json!({ "jobs": [] }))
}

// ── ④ 分散DB統合 (フェデレーション) ─────────────────────────────

#[handler]
fn list_federation(state: Data<&Arc<AdminState>>) -> Json<Value> {
    let sources = state.federation.lock().clone();
    Json(json!({ "sources": sources }))
}

#[handler]
fn register_federation(state: Data<&Arc<AdminState>>, Json(mut src): Json<FederatedSource>) -> Json<Value> {
    src.status = Some("unknown".into());
    let mut list = state.federation.lock();
    if list.iter().any(|s| s.name == src.name) {
        return Json(json!({ "success": false, "message": format!("既に存在します: {}", src.name) }));
    }
    list.push(src);
    Json(json!({ "success": true }))
}

#[handler]
fn federation_test(Json(req): Json<SourceUriRequest>) -> Json<Value> {
    match tcp_reachable(&req.uri) {
        Ok(addr) => Json(json!({ "ok": true, "message": format!("解決OK: {addr}") })),
        Err(e) => Json(json!({ "ok": false, "message": e })),
    }
}

#[handler]
fn drop_federation(state: Data<&Arc<AdminState>>, Json(req): Json<DropRequest>) -> Json<Value> {
    state.federation.lock().retain(|s| s.name != req.name);
    Json(json!({ "success": true }))
}

/// 横断クエリ。`local.*` はローカルエンジンで実行。
/// 外部ソース参照を含む場合はリモート実行が必要なため、現状は受理せずメッセージを返す。
#[handler]
async fn federated_query(state: Data<&Arc<AdminState>>, Json(req): Json<SqlRequest>) -> poem::Result<Json<Value>> {
    let started = Instant::now();

    // 登録済み外部ソースを参照しているか判定
    let sources = state.federation.lock().clone();
    let touches_remote = sources.iter().any(|s| req.sql.contains(&format!("{}.", s.name)));
    if touches_remote {
        return Err(poem::Error::from_string(
            "外部ソースを跨ぐリモート実行は未実装です (コネクタ実装後に有効化)。local.* のみのクエリは実行できます。",
            poem::http::StatusCode::NOT_IMPLEMENTED,
        ));
    }

    // local. プレフィックスを除去してローカル実行 (OLAP は DataFusion 経路)
    let sql = req.sql.replace("local.", "");
    match state.engine.execute_async(&sql).await {
        Ok(QueryResponse::Rows { columns, rows }) => {
            let rows: Vec<Vec<String>> = rows
                .into_iter()
                .map(|r| r.iter().map(SqlValue::as_text).collect())
                .collect();
            Ok(Json(json!({
                "columns": columns,
                "rows": rows,
                "sources_touched": ["local"],
                "elapsed_ms": started.elapsed().as_millis(),
            })))
        }
        Ok(QueryResponse::Command { tag }) => Ok(Json(json!({
            "columns": ["result"],
            "rows": [[tag]],
            "sources_touched": ["local"],
            "elapsed_ms": started.elapsed().as_millis(),
        }))),
        Err(e) => Err(poem::Error::from_string(e, poem::http::StatusCode::BAD_REQUEST)),
    }
}

// ── クラスタ (分散基盤) ────────────────────────────────────────

#[handler]
fn cluster_status(state: Data<&Arc<AdminState>>) -> Json<Value> {
    let commit_count = state.engine.version().log(1_000_000).len() as u64;
    let total_rows = state.engine.total_rows() as u64;
    let table_count = state.engine.table_names().len() as u64;

    let topo = state.topology.lock();
    let alive = topo.alive_nodes();

    // ノード一覧 (トポロジ由来)。Leader 判定は Range のリーダーに含まれるかで近似。
    let nodes: Vec<Value> = topo
        .nodes
        .iter()
        .map(|n| {
            let is_leader = topo.ranges.iter().any(|r| r.leader == n.node_id);
            let range_cnt = topo.ranges.iter().filter(|r| r.replicas.contains(&n.node_id)).count();
            json!({
                "node_id": n.node_id, "addr": n.addr,
                "role": if is_leader { "Leader" } else { "Follower" },
                "alive": n.alive,
                "term": 0, "commit_index": commit_count, "applied_index": commit_count,
                "ranges": range_cnt,
                "disk_used_gb": (total_rows as f64 * 64.0) / 1e9,
                "cpu_pct": 0, "last_heartbeat_ms": 0
            })
        })
        .collect();

    let ranges: Vec<Value> = topo
        .ranges
        .iter()
        .map(|r| {
            json!({
                "range_id": r.range_id,
                "start_key": r.start_key.as_ref().map(|k| String::from_utf8_lossy(k).to_string()).unwrap_or_else(|| "(min)".into()),
                "end_key": r.end_key.as_ref().map(|k| String::from_utf8_lossy(k).to_string()).unwrap_or_else(|| "(max)".into()),
                "leader_node": r.leader, "replicas": r.replicas,
                "size_mb": (r.size_bytes as f64) / 1e6,
            })
        })
        .collect();

    let stats = json!({
        "total_nodes": topo.nodes.len(), "healthy_nodes": alive.len(),
        "total_ranges": topo.range_count(),
        "total_rows": total_rows, "total_disk_gb": (total_rows as f64 * 64.0) / 1e9,
        "raft_term": 0, "replication_factor": topo.replication_factor, "table_count": table_count,
        "under_replicated": topo.under_replicated(),
        "ranges_needing_split": topo.ranges_needing_split(),
    });

    Json(json!({ "stats": stats, "nodes": nodes, "ranges": ranges }))
}

#[handler]
fn cluster_node(state: Data<&Arc<AdminState>>, Json(req): Json<NodeRequest>) -> Json<Value> {
    let mut topo = state.topology.lock();
    match req.action.as_str() {
        "add" | "join" => {
            topo.add_node(req.node_id, req.addr.clone());
            // RF を生存ノード数に合わせて引き上げ (最大3)
            topo.replication_factor = topo.nodes.len().min(3);
            Json(json!({
                "success": true,
                "message": format!("ノード {} ({}) を追加。総ノード数={}", req.node_id, req.addr, topo.nodes.len()),
                "note": "Raft グループへの実参加 (ログ同期) は openraft ネットワーク実装後に有効化されます。"
            }))
        }
        "remove" | "decommission" => {
            topo.set_node_alive(req.node_id, false);
            Json(json!({ "success": true, "message": format!("ノード {} を decommission 候補に設定。", req.node_id) }))
        }
        other => Json(json!({ "success": false, "message": format!("未知のノード操作: {other}") })),
    }
}

#[handler]
fn cluster_rebalance(state: Data<&Arc<AdminState>>) -> Json<Value> {
    let mut topo = state.topology.lock();
    let plan = topo.rebalance_plan();
    if plan.is_empty() {
        return Json(json!({ "success": true, "message": "再配置は不要です (全 Range が replication_factor を満たしています)。", "moves": [] }));
    }
    let moves: Vec<Value> = plan
        .iter()
        .map(|(rid, node)| json!({ "range_id": rid, "add_replica_node": node }))
        .collect();
    // 計画を適用 (メタデータ上のレプリカ割当。実データ移送は Raft 実装後)
    for (rid, node) in &plan {
        topo.add_replica(*rid, *node);
    }
    Json(json!({
        "success": true,
        "message": format!("{} 件のレプリカ再配置を計画・適用しました。", moves.len()),
        "moves": moves,
        "note": "メタデータ上の配置です。実データのレプリカ移送は openraft 複製の実装後に有効化されます。"
    }))
}

// ── Raft ノード間 RPC 受信 ──────────────────────────────────────

/// AppendEntries 受信 (Leader → このノード)
#[handler]
fn raft_append(
    state: Data<&Arc<AdminState>>,
    Json(req): Json<aruaru_dist::AppendEntriesReq>,
) -> Json<Value> {
    let Some(node) = state.cluster_node() else {
        return Json(json!({ "term": 0, "success": false, "match_index": 0, "from": 0 }));
    };
    let result = node.append_entries(
        req.term,
        req.prev_log_index,
        req.prev_log_term,
        req.entries,
        req.leader_commit,
    );
    // commit が進んでいれば適用
    node.apply_committed();
    Json(json!({
        "term": result.term,
        "success": result.success,
        "match_index": result.match_index,
        "from": node.node_id(),
    }))
}

/// RequestVote 受信 (Candidate → このノード)
#[handler]
fn raft_vote(
    state: Data<&Arc<AdminState>>,
    Json(req): Json<aruaru_dist::RequestVoteReq>,
) -> Json<Value> {
    let Some(node) = state.cluster_node() else {
        return Json(json!({ "term": 0, "vote_granted": false, "from": 0 }));
    };
    let result = node.request_vote(
        req.term,
        req.candidate_id,
        req.last_log_index,
        req.last_log_term,
    );
    Json(json!({
        "term": result.term,
        "vote_granted": result.granted,
        "from": node.node_id(),
    }))
}

/// クライアント書き込みを Raft 経由で提案 (Leader のみ受理)
#[handler]
fn cluster_propose(state: Data<&Arc<AdminState>>, Json(req): Json<SqlRequest>) -> Json<Value> {
    let Some(node) = state.cluster_node() else {
        // 非クラスタモード: 通常パスで実行
        return match state.engine.execute(&req.sql) {
            Ok(resp) => Json(json!({ "success": true, "mode": "standalone", "result": crate::cluster::summarize(resp) })),
            Err(e) => Json(json!({ "success": false, "message": e })),
        };
    };
    if node.role() != aruaru_dist::RaftRole::Leader {
        return Json(json!({
            "success": false,
            "message": "not leader",
            "role": format!("{:?}", node.role()),
            "note": "書き込みは Leader ノードへ送ってください。"
        }));
    }
    match crate::cluster::propose_write(&node, &req.sql) {
        Ok(idx) => Json(json!({
            "success": true, "mode": "raft", "log_index": idx,
            "commit_index": node.commit_index(),
            "message": format!("提案を log index {idx} に追加しました。")
        })),
        Err(e) => Json(json!({ "success": false, "message": e })),
    }
}

// ── 対応DBレジストリ (150+件) ───────────────────────────────────

#[handler]
fn registry_list(state: Data<&Arc<AdminState>>) -> Json<Value> {
    // rank 昇順の全エントリ
    let entries = state.registry.all();
    Json(serde_json::to_value(entries).unwrap_or(json!([])))
}

#[handler]
fn registry_summary(state: Data<&Arc<AdminState>>) -> Json<Value> {
    Json(serde_json::to_value(state.registry.summary()).unwrap_or(json!({})))
}

/// 今すぐクロールしてランキングを更新
#[handler]
async fn registry_crawl(state: Data<&Arc<AdminState>>) -> Json<Value> {
    match state.registry.crawl_now().await {
        Ok(report) => Json(json!({ "success": true, "report": report })),
        Err(e) => Json(json!({ "success": false, "message": e.to_string() })),
    }
}

/// レジストリの DB に対する実接続テスト (PG ワイヤ互換のみ実接続、他は能力情報を返す)
#[handler]
async fn registry_test_connection(
    state: Data<&Arc<AdminState>>,
    Json(req): Json<RegistryTestRequest>,
) -> Json<Value> {
    let Some(entry) = state.registry.get(&req.id) else {
        return Json(json!({ "ok": false, "message": format!("未登録のDB: {}", req.id) }));
    };

    match aruaru_registry::adapter::adapter_for(entry.wire) {
        Some(adapter) => {
            let res = adapter.test(&req.uri).await;
            Json(json!({
                "ok": res.ok,
                "message": res.message,
                "server_version": res.server_version,
                "wire": adapter.wire_name(),
            }))
        }
        None => Json(json!({
            "ok": false,
            "message": format!("{} のワイヤ({:?})用アダプタは未実装です。", entry.name, entry.wire),
            "status": entry.status.label(),
        })),
    }
}
