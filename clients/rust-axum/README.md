# Rust + Axum + sqlx (async)

aruaru-db を**標準の sqlx(PgPool)** でそのまま使う非同期サンプル。
**Poem / RPoem** でも同じ `PgPool` をハンドラで受け取るだけ(コードの
DB 部分は変わらない)。同期版は `postgres` crate の `Client::connect`
へ置換(ワイヤ形式は同一、速度差なし)。

## Cargo.toml(抜粋)

```toml
axum = "0.7"
tokio = { version = "1", features = ["full"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres"] }
serde_json = "1"
anyhow = "1"
```

## 実行

```bash
export ARUARU_DB_DSN="postgres://app:secret@localhost:5433/app?sslmode=require"
cargo run   # :8000 で待受
```

## エンドポイント

`POST /items/:id?qty=5&message=...`(UPSERT → `aruaru_commit`)/
`GET /items/:id`(最新)/ `GET /items/:id/at/:commit`(`AS OF COMMIT`)。

## 検証状況

**この例自体は未検証**(ビルドは別ワークスペース前提)。ただし
`sqlx` ↔ aruaru-db pgwire の往復は過去セッションで実 PostgreSQL 相当
環境で検証済み(`../../CLAUDE.md` 2026-07-13/14、拡張プロトコル対応も
その時に修正)。
