//! aruaru-db Web管理UI(2026-07-30新設)。
//!
//! **技術スタック**: Rust + RPoem(`open-runo-poem-compat`、poem互換API面の
//! 薄いファサード、実体はtokio/hyper直接実装であり`poem`/`tauri`パッケージ
//! への直接依存は無い——open-raid-z/web(2026-07-30新設)と同じ設計判断)。
//!
//! **アーキテクチャ**: 既存の`aruaru-server`(`poem`ベース、GraphQL/HTTPポート
//! 側に`/admin/*`管理APIを既に持つ)へのリバースプロキシとして動作する
//! ——open-raid-zの`orzctl`サブプロセス呼び出しと違い、aruaru-serverは
//! 常駐デーモンのため`reqwest`でHTTP経由で呼び出す。**正直な開示**:
//! `aruaru-server`の`/admin/*`ルート自体には認証機構が無い
//! (`aruaru-db/CLAUDE.md`の既知ギャップ、disaster-email-backup系エンド
//! ポイントのみ独自にx-admin-tokenを検証)。そのため本Web層が独自に
//! 管理者トークンゲートを追加する(open-raid-z/webの`OPEN_RAID_Z_
//! ADMIN_TOKEN`と同じ設計)。
//!
//! **2026-07-30追記(セキュリティ強化)**: ユーザー指示「aruaru-serverは
//! 外部から乗っ取られないようにセキュリティをしっかりして」を受け、
//! `aruaru-server`側でも`/admin/*`全体に`x-admin-token`認証を遡及適用
//! した(詳細は`aruaru-db/crates/aruaru-server/src/admin.rs`参照)。
//! そのため本Web層は2つの独立したトークンを扱う: (1)ブラウザ↔本Web層間
//! の`ARUARU_WEB_ADMIN_TOKEN`、(2)本Web層↔aruaru-server間の
//! `ARUARU_UPSTREAM_ADMIN_TOKEN`(aruaru-server起動時の
//! `ARUARU_DB_ADMIN_TOKEN`と同じ値を設定する)。同一値にする必要は無い
//! (多層防御、片方が漏れてももう片方が独立して機能する)。
//!
//! **read-onlyデモ(rs-sync/open-raid-z/webと同じ設計思想)**:
//! `ARUARU_WEB_READ_ONLY=1`環境変数が設定されている場合、クラスタ操作
//! (`POST /api/rebalance`)は管理者トークンの有無に関わらず常に拒否する。

use std::sync::Arc;

use open_runo_poem_compat::hyper_compat::{self, empty_status, html_response, json_response, Params, Request, Response};
use open_runo_poem_compat::{get, post, Route, Server, StatusCode, TcpListener};

struct Config {
    admin_base_url: String,
    admin_token: Option<String>,
    upstream_admin_token: Option<String>,
    read_only: bool,
    http_client: reqwest::Client,
    /// ブラウザ側JSが`fetch()`する際に前置するパスプレフィックス
    /// (2026-08-01追加、実バグ修正)。`open-web-server`の「分身の術」
    /// テナントルーティング(`path_prefix`剥がし転送)配下にマウントする
    /// 場合、ブラウザは絶対パス`/api/...`だと常にオリジン直下を叩いて
    /// しまう——open-redmine/open-gitea/RS-Syncが過去に繰り返し踏んだのと
    /// 全く同じ罠が、実際に`https://easy-web.tokyo/aruaru-db/`への
    /// 実デプロイ後の実ブラウザ確認で再現した(`GET /api/status`ではなく
    /// `GET /aruaru-db/api/status`が呼ばれる必要があった)。
    base_path: String,
}

impl Config {
    fn from_env() -> Self {
        Self {
            admin_base_url: std::env::var("ARUARU_ADMIN_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:4000/admin".to_string()),
            admin_token: std::env::var("ARUARU_WEB_ADMIN_TOKEN").ok(),
            upstream_admin_token: std::env::var("ARUARU_UPSTREAM_ADMIN_TOKEN").ok(),
            read_only: matches!(std::env::var("ARUARU_WEB_READ_ONLY").as_deref(), Ok("1") | Ok("true")),
            http_client: reqwest::Client::new(),
            base_path: std::env::var("ARUARU_WEB_BASE_PATH").unwrap_or_default(),
        }
    }
}

fn page_html(demo: bool, base_path: &str) -> String {
    let banner = if demo {
        r#"<div class="banner demo">これはread-onlyデモです。ログイン・登録・保存(クラスタ操作)は実際には出来ません。</div>"#
    } else {
        ""
    };
    format!(
        r#"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>aruaru-db 管理UI</title>
<style>
  body {{ font-family: system-ui, sans-serif; max-width: 720px; margin: 2rem auto; padding: 0 1rem; }}
  .banner.demo {{ background: #fff3cd; border: 1px solid #ffe69c; padding: .75rem 1rem; border-radius: .375rem; margin-bottom: 1rem; }}
  pre {{ background: #f6f8fa; padding: 1rem; border-radius: .375rem; overflow-x: auto; }}
  section {{ margin-top: 1.5rem; }}
  label {{ display: block; margin-bottom: .3rem; font-size: .85rem; color: #555; }}
  input {{ width: 100%; padding: .5rem; margin-bottom: .75rem; box-sizing: border-box; }}
  button {{ padding: .5rem 1rem; cursor: pointer; }}
</style>
</head>
<body>
{banner}
<h1>aruaru-db 管理UI</h1>
<section>
  <h2>クラスタ状態</h2>
  <button id="refresh">更新</button>
  <pre id="status">(未取得)</pre>
</section>
<section id="admin-section">
  <h2>リバランス実行(管理者のみ)</h2>
  <label for="admin-token">管理者トークン</label>
  <input id="admin-token" type="password" placeholder="X-Admin-Token">
  <button id="rebalance">リバランス実行</button>
  <pre id="rebalance-result"></pre>
</section>
<script>
const BASE_PATH = '{base_path}';
async function refreshStatus() {{
  const el = document.getElementById('status');
  el.textContent = '取得中...';
  try {{
    const res = await fetch(BASE_PATH + '/api/status');
    const body = await res.text();
    el.textContent = body;
  }} catch (e) {{
    el.textContent = 'エラー: ' + e;
  }}
}}
document.getElementById('refresh').addEventListener('click', refreshStatus);
document.getElementById('rebalance').addEventListener('click', async () => {{
  const token = document.getElementById('admin-token').value;
  const el = document.getElementById('rebalance-result');
  el.textContent = '実行中...';
  try {{
    const res = await fetch(BASE_PATH + '/api/rebalance', {{
      method: 'POST',
      headers: {{ 'Content-Type': 'application/json', 'X-Admin-Token': token }},
      body: '{{}}'
    }});
    el.textContent = await res.text();
  }} catch (e) {{
    el.textContent = 'エラー: ' + e;
  }}
}});
refreshStatus();
</script>
</body>
</html>"#
    )
}

fn main() {
    let config = Arc::new(Config::from_env());

    let index_config = Arc::clone(&config);
    let index_handler = std::sync::Arc::new(move |_req: Request, _params: Params| {
        let config = Arc::clone(&index_config);
        Box::pin(async move { html_response(StatusCode::OK, page_html(false, &config.base_path)) }) as hyper_compat::BoxFuture<Response>
    }) as hyper_compat::Handler;
    let demo_config = Arc::clone(&config);
    let demo_handler = std::sync::Arc::new(move |_req: Request, _params: Params| {
        let config = Arc::clone(&demo_config);
        Box::pin(async move { html_response(StatusCode::OK, page_html(true, &config.base_path)) }) as hyper_compat::BoxFuture<Response>
    }) as hyper_compat::Handler;

    let status_config = Arc::clone(&config);
    let status_handler = std::sync::Arc::new(move |_req: Request, _params: Params| {
        let config = Arc::clone(&status_config);
        Box::pin(async move {
            let url = format!("{}/cluster", config.admin_base_url);
            let mut builder = config.http_client.get(&url);
            if let Some(token) = &config.upstream_admin_token {
                builder = builder.header("x-admin-token", token);
            }
            match builder.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    match resp.bytes().await {
                        Ok(bytes) => hyper::Response::builder()
                            .status(hyper::StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY))
                            .header("content-type", "application/json")
                            .body(hyper_compat::fixed_body(bytes))
                            .unwrap(),
                        Err(e) => json_response(StatusCode::BAD_GATEWAY, &serde_json::json!({ "error": format!("failed to read aruaru-server response body: {e}") })),
                    }
                }
                Err(e) => json_response(StatusCode::BAD_GATEWAY, &serde_json::json!({ "error": format!("failed to reach aruaru-server admin API at {url}: {e}") })),
            }
        }) as hyper_compat::BoxFuture<Response>
    }) as hyper_compat::Handler;

    let rebalance_config = Arc::clone(&config);
    let rebalance_handler = std::sync::Arc::new(move |req: Request, _params: Params| {
        let config = Arc::clone(&rebalance_config);
        Box::pin(async move {
            if config.read_only {
                return json_response(StatusCode::FORBIDDEN, &serde_json::json!({ "error": "read-only demo: クラスタ操作は無効化されています / this is a read-only demo, cluster operations are disabled" }));
            }
            let admin_token = match &config.admin_token {
                Some(t) => t.clone(),
                None => return json_response(StatusCode::SERVICE_UNAVAILABLE, &serde_json::json!({ "error": "ARUARU_WEB_ADMIN_TOKEN is not configured on this server" })),
            };
            let provided = req.headers().get("x-admin-token").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            if provided.is_empty() || provided != admin_token {
                return empty_status(StatusCode::UNAUTHORIZED);
            }
            let url = format!("{}/cluster/rebalance", config.admin_base_url);
            let mut builder = config.http_client.post(&url).json(&serde_json::json!({}));
            if let Some(token) = &config.upstream_admin_token {
                builder = builder.header("x-admin-token", token);
            }
            match builder.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    match resp.bytes().await {
                        Ok(bytes) => hyper::Response::builder()
                            .status(hyper::StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY))
                            .header("content-type", "application/json")
                            .body(hyper_compat::fixed_body(bytes))
                            .unwrap(),
                        Err(e) => json_response(StatusCode::BAD_GATEWAY, &serde_json::json!({ "error": format!("failed to read aruaru-server response body: {e}") })),
                    }
                }
                Err(e) => json_response(StatusCode::BAD_GATEWAY, &serde_json::json!({ "error": format!("failed to reach aruaru-server admin API at {url}: {e}") })),
            }
        }) as hyper_compat::BoxFuture<Response>
    }) as hyper_compat::Handler;

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("failed to build tokio runtime");
    rt.block_on(async move {
        let app = Route::new()
            .at("/", get(index_handler))
            .at("/demo", get(demo_handler))
            .at("/api/status", get(status_handler))
            .at("/api/rebalance", post(rebalance_handler));

        let bind_addr: std::net::SocketAddr = std::env::var("ARUARU_WEB_BIND").unwrap_or_else(|_| "127.0.0.1:8098".to_string()).parse().expect("invalid ARUARU_WEB_BIND");
        let (addr, handle) = Server::new(TcpListener::bind(bind_addr)).run(app).await.expect("failed to bind server");
        println!("aruaru-db-web listening on http://{addr} (read_only={})", config.read_only);
        handle.await.ok();
    });
}
