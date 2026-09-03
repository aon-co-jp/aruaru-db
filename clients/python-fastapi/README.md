# Python + FastAPI + asyncpg (async)

aruaru-db を**標準の asyncpg** でそのまま使う非同期サンプル。独自ドライバ
なし。同期版が要るなら `psycopg[binary]` の同期 API へ置換するだけ
(ワイヤ形式は同一、速度差なし)。

## 実行

```bash
pip install fastapi uvicorn asyncpg
export ARUARU_DB_DSN="postgresql://app:secret@localhost:5433/app?ssl=require"
uvicorn app:app --port 8000
```

## エンドポイント

| メソッド | パス | 動作 |
|---|---|---|
| `POST` | `/items/{id}?qty=5&message=...` | UPSERT → `SELECT aruaru_commit(msg)` → `commit_id` を返す |
| `GET` | `/items/{id}` | 最新値 |
| `GET` | `/items/{id}/at/{commit_id}` | `... AS OF COMMIT '<id>'` で過去値 |

## 検証状況

**未検証**(この開発環境に `asyncpg` が未導入)。接続文字列とポート
(5433)以外に aruaru-db 固有の作法は無い。過去に Rust `sqlx` /
`psql` での pgwire 往復は検証済み(`../../CLAUDE.md` 2026-07-13/14)。
