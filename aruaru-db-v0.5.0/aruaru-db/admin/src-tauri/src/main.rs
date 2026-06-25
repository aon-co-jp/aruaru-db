// aruaru-DB Admin GUI (Tauri 2)
// フロントエンド(React/Vite)から Rust コマンド経由で aruaru-server と通信する。
//
// サーバ側は管理用エンドポイント (HTTP/JSON) を以下に公開する想定:
//   POST  {base}/admin/backup            バックアップ作成
//   GET   {base}/admin/backup            一覧
//   POST  {base}/admin/backup/restore    リストア / PITR
//   POST  {base}/admin/migrate/test      移行元接続テスト
//   POST  {base}/admin/migrate/preview   スキーマプレビュー
//   POST  {base}/admin/migrate/run       移行実行
//   GET   {base}/admin/parallel          並列実行設定
//   POST  {base}/admin/parallel          並列実行設定の更新
//   POST  {base}/admin/parallel/explain  分散実行プラン
//   GET   {base}/admin/cluster           クラスタ状態
//   POST  {base}/admin/cluster/node      ノード追加/除去
//   POST  {base}/admin/cluster/rebalance リバランス
//   GET   {base}/admin/federation        統合(フェデレーション)ソース一覧
//   POST  {base}/admin/federation        ソース登録/削除
//   POST  {base}/admin/federation/query  横断クエリ
//
// これらサーバ側ハンドラの実装は aruaru-server の次タスク。
// 本ファイルは「呼び出し規約」を確定させ、フロントを駆動できる状態にする。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tauri::Manager;

// ── HTTP ヘルパ ────────────────────────────────────────────────

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}

async fn admin_get(base: &str, path: &str) -> Result<Value, String> {
    let client = http_client()?;
    client
        .get(format!("{base}{path}"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<Value>()
        .await
        .map_err(|e| e.to_string())
}

async fn admin_post(base: &str, path: &str, body: Value) -> Result<Value, String> {
    let client = http_client()?;
    client
        .post(format!("{base}{path}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<Value>()
        .await
        .map_err(|e| e.to_string())
}

// ── 既存: GraphQL / 接続 ──────────────────────────────────────

#[tauri::command]
async fn graphql_query(
    query: String,
    variables: Option<Value>,
    server_url: String,
) -> Result<Value, String> {
    let client = http_client()?;
    let payload = json!({ "query": query, "variables": variables.unwrap_or(Value::Null) });
    client
        .post(&server_url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<Value>()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn ping_server(url: String) -> Result<bool, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| e.to_string())?;
    Ok(client
        .get(&url)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false))
}

#[tauri::command]
async fn list_branches(server_url: String) -> Result<Vec<Value>, String> {
    let query = r#"query { branches { name headCommitId isCurrent } }"#;
    let result = graphql_query(query.to_string(), None, server_url).await?;
    Ok(result["data"]["branches"].as_array().cloned().unwrap_or_default())
}

#[tauri::command]
async fn get_commit_log(server_url: String, limit: i32) -> Result<Vec<Value>, String> {
    let query = format!(
        r#"query {{ log(limit: {limit}) {{ id shortId author message timestamp rootHash }} }}"#
    );
    let result = graphql_query(query, None, server_url).await?;
    Ok(result["data"]["log"].as_array().cloned().unwrap_or_default())
}

// ── ① バックアップ ─────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupRequest {
    pub kind: String,        // "Full" | "Incremental" | "Snapshot"
    pub dest_type: String,   // "Local" | "S3" | "SFTP"
    pub dest_uri: String,    // パス or s3://bucket/prefix or sftp://...
    pub encrypt: bool,
    pub retention_days: u32,
    pub branch: String,
}

#[tauri::command]
async fn create_backup(base_url: String, req: BackupRequest) -> Result<Value, String> {
    admin_post(&base_url, "/admin/backup", serde_json::to_value(req).unwrap()).await
}

#[tauri::command]
async fn list_backups(base_url: String) -> Result<Vec<Value>, String> {
    let v = admin_get(&base_url, "/admin/backup").await?;
    Ok(v["backups"].as_array().cloned().unwrap_or_default())
}

#[tauri::command]
async fn restore_backup(
    base_url: String,
    backup_id: String,
    target_branch: String,
    point_in_time: Option<String>, // RFC3339; PITR 指定時
) -> Result<Value, String> {
    admin_post(
        &base_url,
        "/admin/backup/restore",
        json!({ "backup_id": backup_id, "target_branch": target_branch, "point_in_time": point_in_time }),
    )
    .await
}

#[tauri::command]
async fn set_backup_schedule(
    base_url: String,
    cron: String,
    enabled: bool,
    kind: String,
) -> Result<Value, String> {
    admin_post(
        &base_url,
        "/admin/backup/schedule",
        json!({ "cron": cron, "enabled": enabled, "kind": kind }),
    )
    .await
}

// ── ② お引越し (移行 / 移植) ───────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct MigrationConfig {
    pub source: String,      // "postgres"|"cockroach"|"snowflake"|"mysql"|"csv"|"parquet"|"aruaru"
    pub source_uri: String,
    pub batch_size: u32,
    pub commit_message: String,
    pub parallel_workers: u32, // 並列取り込みワーカー数
    pub include_tables: Vec<String>, // 空なら全テーブル
}

#[tauri::command]
async fn test_source_connection(base_url: String, source: String, uri: String) -> Result<Value, String> {
    admin_post(&base_url, "/admin/migrate/test", json!({ "source": source, "uri": uri })).await
}

#[tauri::command]
async fn preview_source_schema(base_url: String, source: String, uri: String) -> Result<Value, String> {
    admin_post(&base_url, "/admin/migrate/preview", json!({ "source": source, "uri": uri })).await
}

#[tauri::command]
async fn run_migration(base_url: String, config: MigrationConfig) -> Result<Value, String> {
    admin_post(&base_url, "/admin/migrate/run", serde_json::to_value(config).unwrap()).await
}

/// aruaru → aruaru の「まるごとお引越し」(別クラスタへの移植)
#[tauri::command]
async fn migrate_instance(
    base_url: String,
    target_uri: String,
    include_history: bool, // コミット履歴(Git-on-SQL)も移送するか
) -> Result<Value, String> {
    admin_post(
        &base_url,
        "/admin/migrate/instance",
        json!({ "target_uri": target_uri, "include_history": include_history }),
    )
    .await
}

// ── ③ 分散並列化 ───────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ParallelConfig {
    pub max_parallelism: u32,         // クエリあたり最大並列度
    pub worker_threads_per_node: u32, // ノードあたりワーカースレッド
    pub enable_parallel_scan: bool,
    pub enable_parallel_aggregate: bool,
    pub enable_shuffle_join: bool,
    pub shuffle_partitions: u32,      // シャッフル分割数
    pub broadcast_threshold_mb: u32,  // これ以下は broadcast join
}

#[tauri::command]
async fn get_parallel_config(base_url: String) -> Result<Value, String> {
    admin_get(&base_url, "/admin/parallel").await
}

#[tauri::command]
async fn set_parallel_config(base_url: String, config: ParallelConfig) -> Result<Value, String> {
    admin_post(&base_url, "/admin/parallel", serde_json::to_value(config).unwrap()).await
}

/// 分散実行プラン (どのフラグメントがどのノードで並列に走るか) を取得
#[tauri::command]
async fn explain_distributed(base_url: String, sql: String) -> Result<Value, String> {
    admin_post(&base_url, "/admin/parallel/explain", json!({ "sql": sql })).await
}

/// 実行中の並列ジョブ一覧
#[tauri::command]
async fn list_parallel_jobs(base_url: String) -> Result<Vec<Value>, String> {
    let v = admin_get(&base_url, "/admin/parallel/jobs").await?;
    Ok(v["jobs"].as_array().cloned().unwrap_or_default())
}

// ── ④ 分散DB統合 (フェデレーション) ─────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct FederatedSource {
    pub name: String,
    pub kind: String,   // "aruaru"|"postgres"|"cockroach"|"snowflake"|"mysql"
    pub uri: String,
    pub read_only: bool,
    pub pushdown: bool, // 述語/集計プッシュダウンを許可するか
}

#[tauri::command]
async fn list_federated_sources(base_url: String) -> Result<Vec<Value>, String> {
    let v = admin_get(&base_url, "/admin/federation").await?;
    Ok(v["sources"].as_array().cloned().unwrap_or_default())
}

#[tauri::command]
async fn register_federated_source(base_url: String, source: FederatedSource) -> Result<Value, String> {
    admin_post(&base_url, "/admin/federation", serde_json::to_value(source).unwrap()).await
}

#[tauri::command]
async fn test_federated_source(base_url: String, kind: String, uri: String) -> Result<Value, String> {
    admin_post(&base_url, "/admin/federation/test", json!({ "kind": kind, "uri": uri })).await
}

#[tauri::command]
async fn drop_federated_source(base_url: String, name: String) -> Result<Value, String> {
    admin_post(&base_url, "/admin/federation/drop", json!({ "name": name })).await
}

/// 複数DBをまたぐ横断クエリ (フェデレーテッドクエリ)
#[tauri::command]
async fn federated_query(base_url: String, sql: String) -> Result<Value, String> {
    admin_post(&base_url, "/admin/federation/query", json!({ "sql": sql })).await
}

// ── クラスタ (分散基盤) ────────────────────────────────────────

#[tauri::command]
async fn get_cluster_status(base_url: String) -> Result<Value, String> {
    admin_get(&base_url, "/admin/cluster").await
}

#[tauri::command]
async fn add_cluster_node(base_url: String, node_id: u64, addr: String) -> Result<Value, String> {
    admin_post(&base_url, "/admin/cluster/node", json!({ "action": "add", "node_id": node_id, "addr": addr })).await
}

#[tauri::command]
async fn decommission_node(base_url: String, node_id: u64) -> Result<Value, String> {
    admin_post(&base_url, "/admin/cluster/node", json!({ "action": "decommission", "node_id": node_id })).await
}

#[tauri::command]
async fn rebalance_cluster(base_url: String) -> Result<Value, String> {
    admin_post(&base_url, "/admin/cluster/rebalance", json!({})).await
}

// ── ⑤ 対応DBレジストリ (150+件) ─────────────────────────────────

#[tauri::command]
async fn list_registry(base_url: String) -> Result<Vec<Value>, String> {
    let client = http_client()?;
    let v = client
        .get(format!("{base_url}/admin/registry"))
        .send().await.map_err(|e| e.to_string())?
        .json::<Value>().await.map_err(|e| e.to_string())?;
    Ok(v.as_array().cloned().unwrap_or_default())
}

#[tauri::command]
async fn registry_summary(base_url: String) -> Result<Value, String> {
    admin_get(&base_url, "/admin/registry/summary").await
}

#[tauri::command]
async fn registry_crawl(base_url: String) -> Result<Value, String> {
    admin_post(&base_url, "/admin/registry/crawl", json!({})).await
}

#[tauri::command]
async fn registry_test(base_url: String, id: String, uri: String) -> Result<Value, String> {
    admin_post(&base_url, "/admin/registry/test", json!({ "id": id, "uri": uri })).await
}

// ── Main ──────────────────────────────────────────────────────

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            // 接続/基本
            graphql_query, ping_server, list_branches, get_commit_log,
            // ① バックアップ
            create_backup, list_backups, restore_backup, set_backup_schedule,
            // ② お引越し
            test_source_connection, preview_source_schema, run_migration, migrate_instance,
            // ③ 分散並列化
            get_parallel_config, set_parallel_config, explain_distributed, list_parallel_jobs,
            // ④ 分散DB統合
            list_federated_sources, register_federated_source, test_federated_source,
            drop_federated_source, federated_query,
            // クラスタ
            get_cluster_status, add_cluster_node, decommission_node, rebalance_cluster,
            // ⑤ 対応DBレジストリ
            list_registry, registry_summary, registry_crawl, registry_test,
        ])
        .setup(|app| {
            #[cfg(debug_assertions)]
            {
                if let Some(window) = app.get_webview_window("main") {
                    window.open_devtools();
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
