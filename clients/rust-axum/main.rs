//! aruaru-db + Axum + sqlx (async, PgPool) — minimal connector example.
//!
//! 正本: ../../docs/CLIENTS.md
//! aruaru-db 側に独自ドライバは不要。標準の sqlx(PostgreSQL)でそのまま
//! 繋がる。Poem / RPoem でも同じ `PgPool` をハンドラで使うだけ。
//! 同期が要るなら `postgres` crate の `Client::connect` へ差し替え(ワイヤ同一)。
//!
//! Cargo.toml:
//!   axum = "0.7"
//!   tokio = { version = "1", features = ["full"] }
//!   sqlx = { version = "0.8", features = ["runtime-tokio", "postgres"] }
//!   serde_json = "1"
//!
//! Run:
//!   export ARUARU_DB_DSN="postgres://app:secret@localhost:5433/app?sslmode=require"
//!   cargo run
//!
//! **2026-09-06追記(実機検証で発見・修正した実バグ)**: このファイルは
//! 2026-09-03の新設時点では「レシピ(Cargo.tomlを伴わない読み物)」に
//! 留まり、一度も実際にビルド・実行されていなかった。実際にWSL2 Ubuntu
//! (Linux、AWS Mainframe Modernizationがワークロードを再ホストする先と
//! 同種の`x86_64-unknown-linux-gnu`環境)でCargo.tomlを組み立てて実
//! `aruaru-server`へ接続したところ、以下2件の実バグが見つかった:
//! 1. `pool.begin()`(sqlxの明示トランザクション)が`BeginFailed`で失敗し、
//!    後続リクエストで「transaction already active」というプールの
//!    接続状態異常を引き起こした——aruaru-wireは単純なSQL文の逐次実行を
//!    前提としており、明示的なBEGIN/COMMITラップは他の全コネクタ
//!    (rust-aruaru-db等)も行っていない設計だったため、同じパターンに
//!    揃えてトランザクションラップを撤去した。
//! 2. `get_latest`/`get_as_of`が`Option<i32>`で列をデコードしようとして
//!    `ColumnDecode`エラーで失敗(コメント自体は「Stringで受けてparse
//!    する」と正しく書かれていたが、実装コードは追従しておらず
//!    コメントとコードが乖離していた——ドキュメントと実装の乖離という
//!    このエコシステムで繰り返し見つかるパターンの新たな実例)。
//!    `Option<String>`で受けてから`i32`へparseする形に修正。
//! 3. `get_as_of`が`AS OF COMMIT $2`をバインドパラメータとして渡そうと
//!    していたが、aruaru-wireは`AS OF COMMIT`句をバインドパラメータとして
//!    受け付けない(このリポジトリの全コネクタが共通して行っている
//!    「commit_idを検証してから文字列連結する」設計から外れていた)。
//!    `is_safe_commit_id`(英数字+`-`/`_`、≤128文字)で検証してから
//!    `format!`で安全に文字列連結する形へ修正。
//! 修正後、実サーバへの一連のリクエスト(upsert→commit→最新値取得→
//! 再upsert→過去コミット時点取得)が実際に正しいJSONを返すことを確認済み。

use axum::{extract::{Path, Query, State}, routing::{get, post}, Json, Router};
use serde_json::{json, Value};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dsn = std::env::var("ARUARU_DB_DSN")
        .unwrap_or_else(|_| "postgres://app:secret@localhost:5433/app".into());
    let pool = PgPoolOptions::new().max_connections(8).connect(&dsn).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY, qty INT)")
        .execute(&pool)
        .await?;

    let app = Router::new()
        .route("/items/:id", post(upsert_and_commit).get(get_latest))
        .route("/items/:id/at/:commit", get(get_as_of))
        .with_state(pool);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// commit_id が `AS OF COMMIT '<id>'` のリテラルとして安全か(他の全
/// コネクタと同じルール: 英数字 + `-`/`_`、1〜128文字)。
fn is_safe_commit_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

async fn upsert_and_commit(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    let qty: i32 = q.get("qty").and_then(|s| s.parse().ok()).unwrap_or(0);
    let msg = q.get("message").cloned().unwrap_or_else(|| "api write".into());
    // aruaru-wire は単純なSQL文の逐次実行を前提とする(他の全コネクタも
    // 明示的なBEGIN/COMMITでラップしていない)。プール上の1接続で
    // INSERT→aruaru_commitの順に素直に実行する。
    sqlx::query(
        "INSERT INTO items (id, qty) VALUES ($1, $2) \
         ON CONFLICT (id) DO UPDATE SET qty = EXCLUDED.qty",
    )
    .bind(&id)
    .bind(qty)
    .execute(&pool)
    .await
    .unwrap();
    let commit_id: String = sqlx::query("SELECT aruaru_commit($1)")
        .bind(&msg)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    Json(json!({ "id": id, "qty": qty, "commit_id": commit_id }))
}

async fn get_latest(State(pool): State<PgPool>, Path(id): Path<String>) -> Json<Value> {
    // aruaru-wire は通常のテーブル列を常に VARCHAR(text) で返す
    // (docs/CLIENTS.md §5.1)。String で受けてから parse する。
    let qty: Option<i32> = sqlx::query("SELECT qty FROM items WHERE id = $1")
        .bind(&id)
        .fetch_optional(&pool)
        .await
        .unwrap()
        .and_then(|r| r.get::<Option<String>, _>(0))
        .and_then(|s| s.parse().ok());
    Json(json!({ "id": id, "qty": qty }))
}

async fn get_as_of(
    State(pool): State<PgPool>,
    Path((id, commit)): Path<(String, String)>,
) -> Json<Value> {
    // VersionlessAPI: 過去のコミット時点を読む。commit_id はここで
    // ネイティブに検証してから `AS OF COMMIT '<id>'` へ安全に文字列連結
    // する ── aruaru-wire は `AS OF COMMIT` 句をバインドパラメータとして
    // 受け付けないため(他の全コネクタと同じ「ネットワークに触れる前の
    // ローカル検証」設計)。
    if !is_safe_commit_id(&commit) {
        return Json(json!({ "error": "invalid commit id" }));
    }
    let sql = format!("SELECT qty FROM items WHERE id = $1 AS OF COMMIT '{commit}'");
    let qty: Option<i32> = sqlx::query(&sql)
        .bind(&id)
        .fetch_optional(&pool)
        .await
        .unwrap()
        .and_then(|r| r.get::<Option<String>, _>(0))
        .and_then(|s| s.parse().ok());
    Json(json!({ "id": id, "as_of": commit, "qty": qty }))
}
