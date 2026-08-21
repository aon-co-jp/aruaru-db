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
use poem::{get, handler, post, web::Data, web::Json, Endpoint, EndpointExt, Request, Route};
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
    /// スタンドアロンのメール・ディザスタバックアップ(`disaster_email_backup`
    /// feature有効時のみ実際に構築可能。VPS同期・Raftクラスタ構成の登録
    /// 有無に関わらず、メールアドレスひとつだけで有効化できる最後の砦)。
    #[cfg(feature = "disaster_email_backup")]
    disaster_email_backup: Mutex<Option<Arc<aruaru_dist::DisasterEmailBackup>>>,
    /// 稼働中のRaft複製書き込み経路(pgwireサーバへ渡されているのと同一の
    /// `Arc<dyn ReplicatedWriter>`、2026-07-25追記・gap (b)/(c) 対応)。
    /// `main.rs`でRaftクラスタ構築に成功した場合のみ`Some`。管理API
    /// (`POST /admin/disaster-email-backup`)がここへ`set_disaster_email_backup`
    /// を呼ぶことで、稼働中のインスタンスへ実際に注入できる。また
    /// `cluster_propose`(REST `/admin/cluster/propose`)もこの経路を優先して
    /// 使うことで、`RaftNode`を直接叩く旧経路(`cluster::propose_write`)を
    /// 迂回しなくなる(=disaster-backup配線を必ず経由する)。
    replicator: Mutex<Option<Arc<dyn aruaru_dist::ReplicatedWriter>>>,
    /// 【2026-08-21新設・Vitess Reshard/VTGate scatter-gatherの実配線】
    /// `aruaru-dist::MultiRaftCluster`(Range単位の独立Raftグループ+
    /// キー空間トポロジ)を実際に保持する。既存の`cluster`(単一の
    /// `ClusterNode`、pgwire/GraphQL/REST書き込みが実際に使う本番経路)とは
    /// **独立した並行コンポーネント**として`main.rs`起動時に単一ノード構成
    /// (`MultiRaftCluster::single_node`)で初期化する——既存のOLTP書き込み
    /// 経路には一切影響を与えないオプトイン方式(ユーザー指示「既存機能を
    /// 壊さないことを優先」に基づく選択)。`POST /admin/multi-raft/split`・
    /// `POST /admin/multi-raft/merge`・`GET /admin/multi-raft/scatter-query`
    /// から実際に呼び出せる。
    multi_raft: Mutex<Option<Arc<aruaru_dist::MultiRaftCluster<crate::cluster::EngineApplier>>>>,
    /// 【2026-08-21新設・ScyllaDB shard-per-coreストアの実配線】
    /// `aruaru-query::sharded_store::ShardedRowStore<String>`を実際に
    /// 保持する。既存の`QueryEngine::tables`(`parking_lot::RwLock`)を
    /// 置き換えるのはRaft/Prolly Tree/OLAPキャッシュ全体への影響が大きい
    /// ため見送り、`POST /admin/sharded-store`・`GET
    /// /admin/sharded-store/{key}`という**オプトインの独立ストレージ経路**
    /// として公開する(ユーザー指示「無理のない方を選んでよい」に基づく
    /// 選択)。値は文字列固定(JSON文字列をそのまま保持、`ShardedRowStore<V>`
    /// の型パラメータを固定した最小構成)。
    sharded_store: aruaru_query::sharded_store::ShardedRowStore<String>,
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
            #[cfg(feature = "disaster_email_backup")]
            disaster_email_backup: Mutex::new(None),
            replicator: Mutex::new(None),
            multi_raft: Mutex::new(None),
            // shard_count=0 -> このマシンの論理コア数を自動採用
            // (ScyllaDBの既定「コア数と同数のシャード」を踏襲、`sharded_store.rs`docコメント参照)。
            sharded_store: aruaru_query::sharded_store::ShardedRowStore::new(0),
        })
    }

    /// `MultiRaftCluster`を取り付ける(`main.rs`起動時、単一ノード構成で初期化)。
    pub fn attach_multi_raft(&self, cluster: Arc<aruaru_dist::MultiRaftCluster<crate::cluster::EngineApplier>>) {
        *self.multi_raft.lock() = Some(cluster);
    }

    pub fn multi_raft(&self) -> Option<Arc<aruaru_dist::MultiRaftCluster<crate::cluster::EngineApplier>>> {
        self.multi_raft.lock().clone()
    }

    /// Raft ノードを取り付ける (クラスタモード起動時)
    pub fn attach_cluster(&self, node: Arc<ClusterNode>) {
        *self.cluster.lock() = Some(node);
    }

    pub fn cluster_node(&self) -> Option<Arc<ClusterNode>> {
        self.cluster.lock().clone()
    }

    /// 稼働中のRaft複製書き込み経路(pgwireサーバへ渡すのと同じインスタンス)
    /// を取り付ける。`main.rs`でRaftクラスタ構築に成功した場合のみ呼ばれる。
    pub fn attach_replicator(&self, replicator: Arc<dyn aruaru_dist::ReplicatedWriter>) {
        *self.replicator.lock() = Some(replicator);
    }

    pub fn replicator(&self) -> Option<Arc<dyn aruaru_dist::ReplicatedWriter>> {
        self.replicator.lock().clone()
    }
}

// ── 管理API認証(`x-admin-token`、2026-07-25追記・2026-07-30遡及適用) ──
//
// このリポジトリの`/admin/*`は元々認証機構を持たなかった
// (2026-07-24 HANDOFF「管理API自体に認証機構が現状無い」で明記済みの
// 既知のギャップ)。当初はディザスタ・メールバックアップ管理API限定で
// `open-web-server`/`open-easy-web`と同じ「`x-admin-token`ヘッダー +
// 環境変数設定のトークン、未設定なら503」という規約を導入していたが、
// 2026-07-30、ユーザー指示「aruaru-serverは外部から乗っ取られないように
// セキュリティをしっかりして」を受け、**`admin_routes()`が返すRoute全体を
// `.around()`ミドルウェアで包み、`/admin/*`配下の全エンドポイント
// (cluster/backup/migrate/federation/registry/raftを含む)に同じ認証を
// 遡及適用した**(`aruaru-db/CLAUDE.md`の2026-07-25(続き2)エントリが
// 「次回候補(b)」として明記していた項目そのもの)。`main.rs`側の
// GraphQLエンドポイント(`/graphql`)には適用しない(既存のGraphQL認証
// 方針は別軸のため、今回のスコープ外)。
const ADMIN_TOKEN_ENV: &str = "ARUARU_DB_ADMIN_TOKEN";

/// 定数時間文字列比較(2026-07-30追記、ユーザー指示「高速なランダム要素を
/// 入れた暗号化とセキュリティをしっかりして」への対応の一つ)。
///
/// 素の`==`/`!=`によるトークン比較は、多くの言語処理系で先頭バイトから
/// 逐次比較し不一致を検出した時点で早期リターンするため、**一致した
/// 先頭バイト数に応じてわずかに応答時間が変化する**(タイミングサイド
/// チャネル攻撃、CWE-208として知られる既知の脆弱性クラス)。外部から
/// 到達可能な`/admin/*`の認証トークン比較にこの種の脆弱性を持ち込まない
/// よう、全バイトを必ず走査してから結果をXOR累積で判定する定数時間実装
/// に置き換えた。新規crateへの依存追加(`subtle`等)は行わず、この用途に
/// 限定した最小実装とした。長さが異なる場合も早期リターンせず、常に
/// `expected`の全長を走査する(長さの違いによるタイミング差も避ける)。
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

fn check_admin_auth(req: &Request) -> Result<(), (poem::http::StatusCode, &'static str)> {
    let Ok(expected) = std::env::var(ADMIN_TOKEN_ENV) else {
        return Err((
            poem::http::StatusCode::SERVICE_UNAVAILABLE,
            "admin API is not configured (ARUARU_DB_ADMIN_TOKEN is not set)",
        ));
    };
    let provided = req
        .headers()
        .get("x-admin-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if provided.is_empty() || !constant_time_eq(provided, &expected) {
        return Err((poem::http::StatusCode::UNAUTHORIZED, "invalid or missing x-admin-token header"));
    }
    Ok(())
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
    #[allow(unused_mut)]
    let mut route = Route::new();
    #[cfg(feature = "disaster_email_backup")]
    {
        route = route
            .at("/disaster-email-backup", post(set_disaster_email_backup))
            .at("/disaster-email-backup/verify", post(verify_disaster_email_backup));
    }
    route
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
        // 【2026-08-21新設】ephemeral SQL pod (計算資源の使い捨てプロセス分離)
        .at("/ephemeral-query", post(ephemeral_query))
        // 【2026-08-21新設・実配線】Vitess Reshard(併合)+ VTGate scatter-gather
        .at("/multi-raft/split", post(multi_raft_split))
        .at("/multi-raft/merge", post(multi_raft_merge))
        .at("/multi-raft/scatter-query", get(multi_raft_scatter_query))
        // 【2026-08-21新設・実配線】ScyllaDB shard-per-coreストア
        .at("/sharded-store", post(sharded_store_put))
        .at("/sharded-store/:key", get(sharded_store_get))
        .at("/sharded-store-stats", get(sharded_store_stats))
        .at("/registry", get(registry_list))
        .at("/registry/summary", get(registry_summary))
        .at("/registry/crawl", post(registry_crawl))
        .at("/registry/test", post(registry_test_connection))
        // Raft ノード間 RPC 受信エンドポイント
        .at("/raft/append", post(raft_append))
        .at("/raft/vote", post(raft_vote))
        .data(state)
        .around(|ep, req| async move {
            if let Err((status, msg)) = check_admin_auth(&req) {
                return Ok(poem::Response::builder().status(status).body(msg));
            }
            ep.call(req).await.map(poem::IntoResponse::into_response)
        })
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
///
/// **2026-07-25追記(gap (c) 対応)**: 以前はここで`crate::cluster::
/// propose_write`(`RaftNode`を直接叩き、`propose`→`try_commit_to`→
/// `maybe_commit`→`apply_committed`を手動で行う経路)を呼んでおり、
/// `RaftWriter`(および`disaster_email_backup`配線)を完全に迂回していた。
/// 稼働中の`replicator`(`Arc<dyn ReplicatedWriter>`、pgwireサーバに渡して
/// いるのと同一インスタンス)が取り付けられていれば、そちらを優先して使う
/// ことで、この管理API経由の書き込みもRaftWriterの`propose_and_wait`
/// (=quorum障害時のdisaster-backup配線)を必ず経由するようにした。
/// `replicator`が無い(クラスタ構築に失敗した等の異常系)場合のみ、
/// 後方互換のため旧経路へフォールバックする。
#[handler]
async fn cluster_propose(state: Data<&Arc<AdminState>>, Json(req): Json<SqlRequest>) -> Json<Value> {
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

    if let Some(replicator) = state.replicator() {
        return match replicator.write_sql(&req.sql).await {
            Ok(tag) => Json(json!({
                "success": true, "mode": "raft", "commit_index": node.commit_index(),
                "message": format!("RaftWriter経由で提案・commit+適用が完了しました({tag})。")
            })),
            Err(e) => Json(json!({ "success": false, "message": e })),
        };
    }

    // フォールバック: replicator 未取り付け(構築失敗等の異常系)のみ、
    // RaftNode を直接叩く旧経路を使う。この経路は disaster-backup 配線を
    // 経由しない既知の限界(CLAUDE.md 参照)。
    match crate::cluster::propose_write(&node, &req.sql) {
        Ok(idx) => Json(json!({
            "success": true, "mode": "raft_fallback_no_replicator", "log_index": idx,
            "commit_index": node.commit_index(),
            "message": format!("提案を log index {idx} に追加しました(replicator未取り付けのためRaftNode直接経路、disaster-backup配線対象外)。")
        })),
        Err(e) => Json(json!({ "success": false, "message": e })),
    }
}

// ── Vitess Reshard(併合)+ VTGate scatter-gather の実配線(2026-08-21) ──
//
// `aruaru_dist::MultiRaftCluster`(Range単位の独立Raftグループ+キー空間
// トポロジ)を、`main.rs`起動時に単一ノード構成で初期化し`AdminState`へ
// 取り付けたものを実際にHTTP経由で操作する。既存の本番書き込み経路
// (`cluster_propose`が使う`ClusterNode`/`replicator`)とは独立した並行
// コンポーネント——`multi_raft`が未初期化(構築失敗等)の場合は503を返す。

#[derive(Debug, Deserialize)]
struct MultiRaftSplitRequest {
    range_id: u64,
    /// 分割キー(UTF-8文字列として受け取り、バイト列へ変換して使う)
    split_key: String,
}

/// `POST /admin/multi-raft/split` — CockroachDB方式のRange分割を実際に
/// 実行する(既存の`MultiRaftCluster::split`をHTTP経由で呼べるようにする)。
#[handler]
fn multi_raft_split(state: Data<&Arc<AdminState>>, Json(req): Json<MultiRaftSplitRequest>) -> Json<Value> {
    let Some(cluster) = state.multi_raft() else {
        return Json(json!({
            "success": false,
            "message": "MultiRaftCluster未初期化です(main.rsでの構築に失敗している可能性があります)。"
        }));
    };
    let applier = crate::cluster::EngineApplier::new(state.engine.clone());
    match cluster.split(req.range_id, req.split_key.into_bytes(), applier) {
        Some(new_range_id) => Json(json!({
            "success": true, "new_range_id": new_range_id, "range_count": cluster.range_count(),
        })),
        None => Json(json!({ "success": false, "message": format!("range_id {} が見つかりません", req.range_id) })),
    }
}

#[derive(Debug, Deserialize)]
struct MultiRaftMergeRequest {
    range_a: u64,
    range_b: u64,
}

/// `POST /admin/multi-raft/merge` — Vitess Reshard(併合方向)を実際に
/// 実行する(`ClusterTopology::merge_ranges`+`MultiRaftCluster::merge`を
/// HTTP経由で呼べるようにする、`CLAUDE.md`2026-08-21(続き2)HANDOFF参照)。
#[handler]
fn multi_raft_merge(state: Data<&Arc<AdminState>>, Json(req): Json<MultiRaftMergeRequest>) -> Json<Value> {
    let Some(cluster) = state.multi_raft() else {
        return Json(json!({
            "success": false,
            "message": "MultiRaftCluster未初期化です(main.rsでの構築に失敗している可能性があります)。"
        }));
    };
    match cluster.merge(req.range_a, req.range_b) {
        Some(merged_id) => Json(json!({
            "success": true, "merged_range_id": merged_id, "range_count": cluster.range_count(),
        })),
        None => Json(json!({
            "success": false,
            "message": format!("range {} と {} は併合できません(隣接していないか、存在しません)", req.range_a, req.range_b),
        })),
    }
}

/// `GET /admin/multi-raft/scatter-query` — VTGate scatter-gatherを実際に
/// 実行する(全Rangeのcommit_indexをrange_id順に集約して返す)。
#[handler]
fn multi_raft_scatter_query(state: Data<&Arc<AdminState>>) -> Json<Value> {
    let Some(cluster) = state.multi_raft() else {
        return Json(json!({
            "success": false,
            "message": "MultiRaftCluster未初期化です(main.rsでの構築に失敗している可能性があります)。"
        }));
    };
    let gathered = cluster.scatter_gather(|node| {
        json!({ "commit_index": node.commit_index(), "role": format!("{:?}", node.role()) })
    });
    let ranges: Vec<Value> = gathered
        .into_iter()
        .map(|(range_id, v)| json!({ "range_id": range_id, "reading": v }))
        .collect();
    Json(json!({ "success": true, "range_count": ranges.len(), "ranges": ranges }))
}

// ── ScyllaDB shard-per-coreストアの実配線(2026-08-21) ──────────────
//
// `AdminState::sharded_store`(`ShardedRowStore<String>`)へ実際に
// HTTP経由で読み書きする。専用シャードスレッドとの通信は`std::sync::
// mpsc`のみ(`sharded_store.rs`のdocコメント参照)——このハンドラ自体は
// tokioの非同期タスク上で動くが、`mpsc::Receiver::recv()`はブロッキング
// なので`tokio::task::spawn_blocking`で包み、tokioワーカースレッドを
// 占有しないようにする。

#[derive(Debug, Deserialize)]
struct ShardedStorePutRequest {
    key: String,
    value: String,
}

/// `POST /admin/sharded-store` — 指定キーをShardedRowStoreへ書き込む。
/// 実際にキーのSHA-256から担当シャードを決定し、そのシャード専用スレッド
/// (`std::sync::mpsc`経由)へ値を送って書き込む。
#[handler]
async fn sharded_store_put(state: Data<&Arc<AdminState>>, Json(req): Json<ShardedStorePutRequest>) -> Json<Value> {
    let state = state.0.clone();
    let shard_id = state.sharded_store.shard_for(req.key.as_bytes());
    tokio::task::spawn_blocking(move || {
        state.sharded_store.put(req.key.clone().into_bytes(), req.value.clone());
        json!({ "success": true, "key": req.key, "shard_id": shard_id })
    })
    .await
    .map(Json)
    .unwrap_or_else(|e| Json(json!({ "success": false, "message": e.to_string() })))
}

/// `GET /admin/sharded-store/{key}` — 指定キーをShardedRowStoreから読む。
#[handler]
async fn sharded_store_get(state: Data<&Arc<AdminState>>, poem::web::Path(key): poem::web::Path<String>) -> Json<Value> {
    let state = state.0.clone();
    let shard_id = state.sharded_store.shard_for(key.as_bytes());
    let key_for_get = key.clone();
    let value = tokio::task::spawn_blocking(move || state.sharded_store.get(key_for_get.as_bytes()))
        .await
        .unwrap_or(None);
    Json(json!({ "success": true, "key": key, "shard_id": shard_id, "found": value.is_some(), "value": value }))
}

/// `GET /admin/sharded-store-stats` — 各シャードのエントリ数(ScyllaDBの
/// "hot shard"検知に相当する観測用エンドポイント)を返す。
#[handler]
async fn sharded_store_stats(state: Data<&Arc<AdminState>>) -> Json<Value> {
    let state = state.0.clone();
    tokio::task::spawn_blocking(move || {
        let shard_count = state.sharded_store.shard_count();
        let per_shard: Vec<usize> = (0..shard_count).map(|i| state.sharded_store.shard_len(i)).collect();
        let total: usize = per_shard.iter().sum();
        json!({ "success": true, "shard_count": shard_count, "per_shard_len": per_shard, "total_len": total })
    })
    .await
    .map(Json)
    .unwrap_or_else(|e| Json(json!({ "success": false, "message": e.to_string() })))
}

/// 【2026-08-21新設・ephemeral SQL pod化】指定テナントのテーブルを
/// 独立した子プロセス(`--ephemeral-worker`)へスナップショットとして渡し、
/// その子プロセス内だけで SQL を1回実行させる。子プロセスは応答後に必ず
/// 終了する(`crate::ephemeral_pod::run_ephemeral_query`のdoc参照)。
/// 書き込みは子プロセスのインメモリ上でのみ完結し、親プロセスの永続状態
/// (fjall)には反映されない(意図的な制約、ephemeral_pod.rsのdoc参照)。
#[derive(Debug, Deserialize)]
struct EphemeralQueryRequest {
    tenant_id: String,
    /// 子プロセスへ渡すテーブル名一覧(親プロセスの現在の状態から
    /// スナップショットして渡す)。
    tables: Vec<String>,
    sql: String,
}

#[handler]
async fn ephemeral_query(
    state: Data<&Arc<AdminState>>,
    Json(req): Json<EphemeralQueryRequest>,
) -> Json<Value> {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return Json(json!({ "success": false, "message": format!("current_exe() failed: {e}") })),
    };
    let tables = crate::ephemeral_pod::snapshot_for_tenant(&state.engine, &req.tables);
    let ephemeral_req = crate::ephemeral_pod::EphemeralRequest {
        tenant_id: req.tenant_id.clone(),
        tables,
        sql: req.sql,
    };
    match crate::ephemeral_pod::run_ephemeral_query(&exe, &ephemeral_req).await {
        Ok(resp) => Json(json!({
            "success": resp.ok,
            "tenant_id": req.tenant_id,
            "result": resp.result,
            "error": resp.error,
            "mode": "ephemeral_process",
        })),
        Err(e) => Json(json!({ "success": false, "message": format!("ephemeral worker process failed: {e}") })),
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

// ── スタンドアロンのメール・ディザスタバックアップ(2026-07-25追記) ──
//
// `open-web-server`の`crates/open-web-server-gateway/src/handlers/
// disaster_email_backup.rs`と同じ設計思想: VPS間分散同期・Raftクラスタ
// 構成(`ClusterTopology`/`multi_raft`)・ZFSスナップショット連携
// (`snapshot_pairing`/`open_raid_z`機能)のいずれも一切設定しなくても、
// メールアドレスひとつだけで有効化できるディザスタ・セーフティネット。

#[cfg(feature = "disaster_email_backup")]
#[handler]
fn set_disaster_email_backup(
    req: &Request,
    state: Data<&Arc<AdminState>>,
    Json(config): Json<aruaru_dist::DisasterEmailBackupConfig>,
) -> poem::Result<Json<Value>> {
    if let Err((status, msg)) = check_admin_auth(req) {
        return Err(poem::Error::from_string(msg, status));
    }

    let backup = Arc::new(aruaru_dist::DisasterEmailBackup::new(config));
    *state.disaster_email_backup.lock() = Some(backup.clone());

    // gap (b) 対応: 検証・保管だけでなく、実際に稼働中の RaftWriter
    // (pgwire サーバへ渡しているのと同一インスタンス)へ注入する。
    // クラスタ構築(Raft)に成功していない場合(単一ノード・レプリケータ
    // 無し)は `replicator` が `None` のままなので、その旨を正直に message
    // へ含める。
    let injected = match state.replicator() {
        Some(replicator) => {
            replicator.set_disaster_email_backup(backup);
            true
        }
        None => false,
    };

    Ok(Json(json!({
        "message_ja": if injected {
            "ディザスタ用メール退避先を設定し、稼働中のRaft書き込み経路へ反映しました(他の同期・レプリケーション設定は不要です)。".to_string()
        } else {
            "ディザスタ用メール退避先を設定しましたが、Raftクラスタが構築されていないため稼働中の書き込み経路への反映はできませんでした(--peers 未指定または構築失敗)。設定自体は保持しています。".to_string()
        },
        "message_en": if injected {
            "Disaster email backup destination configured and injected into the live Raft write path (no other sync/replication setup required)."
        } else {
            "Disaster email backup destination configured, but no live Raft write path exists yet (cluster not built), so it could not be injected. The configuration is retained."
        },
        "injected_into_live_replicator": injected,
    })))
}

/// SMTP接続の疎通確認のみ(実送信は行わない)。
#[cfg(feature = "disaster_email_backup")]
#[handler]
async fn verify_disaster_email_backup(req: &Request, state: Data<&Arc<AdminState>>) -> poem::Result<Json<Value>> {
    if let Err((status, msg)) = check_admin_auth(req) {
        return Err(poem::Error::from_string(msg, status));
    }

    let Some(backup) = state.disaster_email_backup.lock().clone() else {
        return Err(poem::Error::from_string(
            "disaster email backup is not configured yet",
            poem::http::StatusCode::NOT_FOUND,
        ));
    };

    match tokio::task::spawn_blocking(move || backup.ensure_ready()).await {
        Ok(Ok(())) => Ok(Json(json!({
            "message_ja": "SMTP接続を確認できました。",
            "message_en": "SMTP connectivity check succeeded.",
        }))),
        Ok(Err(e)) => Err(poem::Error::from_string(
            format!("SMTP connectivity check failed: {e}"),
            poem::http::StatusCode::SERVICE_UNAVAILABLE,
        )),
        Err(e) => Err(poem::Error::from_string(
            format!("verification task panicked: {e}"),
            poem::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}
