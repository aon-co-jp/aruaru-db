# `aruaru-db-connector` — 公式 Rust コネクタ(薄いラッパー)

**これは独自の PostgreSQL ドライバではない。** 業界標準の非同期クライアント
`tokio-postgres` をそのまま使い、その上に aruaru-db の Git-on-SQL 機能を
Rust の慣用 API で足しただけの薄い層。RPoem / Axum / Poem など tokio ベースの
アプリ向け。

正本: [`../../docs/CLIENTS.md`](../../docs/CLIENTS.md)

## 何が増えるか

| メソッド | 実体 |
|---|---|
| `AruaruDb::connect(dsn)` | `tokio_postgres::connect` + 接続タスクを `tokio::spawn` |
| `db.execute` / `db.query` | `tokio_postgres::Client` の薄い透過 |
| `db.commit(message) -> String` | `SELECT aruaru_commit($1)` → commit_id |
| `db.query_as_of(base_select, commit_id, params)` | `base_select` の末尾へ ` AS OF COMMIT '<id>'` を安全に付与(commit_id は `is_safe_commit_id` で検証、SQL インジェクション防止) |
| `db.query_as_of_opt(...)` | 上の 0/1 行版 |
| `db.client()` | 内部の `tokio_postgres::Client`(透過的に何でも) |

## RPoem / Poem で使う

RPoem(`open-runo-poem-compat`)は **Web フレームワーク**であって DB
クライアントを指定しない。「Poem 用ドライバ」は不要:

```rust
// 起動時
let db = std::sync::Arc::new(aruaru_db_connector::AruaruDb::connect(&dsn).await?);

// RPoem のハンドラ登録クロージャで Arc をキャプチャ(RPoem に Data<T> 抽出子は無い)
let db_for_handler = db.clone();
route.at("/items/:id", post(handler_fn(move |req, params| {
    let db = db_for_handler.clone();
    Box::pin(async move {
        let commit = db.commit("api write").await.unwrap();
        // ...
    })
})));
```

プールが要るなら `deadpool-postgres` / `bb8` で包むか、`sqlx::PgPool` を
直接使い `AruaruDb::from_client` は使わず sqlx でクエリを書く(どちらでも
ワイヤは同一、速度差なし)。

## 同期が要る場合

`postgres` crate(`tokio-postgres` の同期ファサード)を直接使う。この
コネクタは非同期専用。

## ビルド・テスト

```bash
cargo test                     # 2 unit + 1 doctest(ネットワーク不要)
# 実サーバ相手の往復:
ARUARU_DB_TEST_DSN="host=localhost port=5433 user=app password=secret dbname=app" \
  cargo test -- --ignored live_commit_and_as_of_round_trip
```

## 検証状況

- **ネットワーク不要のテスト(2 + doctest)= このセッションで green**。
- **実サーバ往復(`--ignored`)= 未実施**(この環境に稼働中 aruaru-server
  なし)。ロジックは `tokio-postgres` の標準 API + 文字列組み立てのみ。
  過去に `sqlx` / `psql` の pgwire 往復は検証済み(`../../CLAUDE.md`
  2026-07-13/14)。
