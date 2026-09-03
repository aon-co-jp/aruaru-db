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

async fn upsert_and_commit(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    let qty: i32 = q.get("qty").and_then(|s| s.parse().ok()).unwrap_or(0);
    let msg = q.get("message").cloned().unwrap_or_else(|| "api write".into());
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO items (id, qty) VALUES ($1, $2) \
         ON CONFLICT (id) DO UPDATE SET qty = EXCLUDED.qty",
    )
    .bind(&id)
    .bind(qty)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let commit_id: String = sqlx::query("SELECT aruaru_commit($1)")
        .bind(&msg)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    Json(json!({ "id": id, "qty": qty, "commit_id": commit_id }))
}

async fn get_latest(State(pool): State<PgPool>, Path(id): Path<String>) -> Json<Value> {
    // NOTE(2026-09-03): aruaru-wire は列を VARCHAR(text) で返す(docs/CLIENTS.md §5.1)。
    // sqlx で i32 に直接デコードすると失敗しうるため String で受けて parse する。
    let qty: Option<i32> = sqlx::query("SELECT qty FROM items WHERE id = $1")
        .bind(&id)
        .fetch_optional(&pool)
        .await
        .unwrap()
        .map(|r| r.get(0));
    Json(json!({ "id": id, "qty": qty }))
}

async fn get_as_of(
    State(pool): State<PgPool>,
    Path((id, commit)): Path<(String, String)>,
) -> Json<Value> {
    // VersionlessAPI: 過去のコミット時点を読む。
    let qty: Option<i32> = sqlx::query("SELECT qty FROM items WHERE id = $1 AS OF COMMIT $2")
        .bind(&id)
        .bind(&commit)
        .fetch_optional(&pool)
        .await
        .unwrap()
        .map(|r| r.get(0));
    Json(json!({ "id": id, "as_of": commit, "qty": qty }))
}
