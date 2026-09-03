# `aruaru-db` — 公式 Python コネクタ(薄いラッパー)

**独自の PostgreSQL ドライバではない。** 標準の `asyncpg`(非同期)/
`psycopg` v3(同期)をそのまま使い、その上に Git-on-SQL を Python の慣用
API で足すだけ。

正本: [`../../docs/CLIENTS.md`](../../docs/CLIENTS.md)

## インストール

```bash
pip install aruaru-db[async]   # FastAPI 等(asyncpg)
pip install aruaru-db[sync]    # Django / Flask 等(psycopg[binary])
```

## 何が増えるか

| API(async / sync 共通) | 実体 |
|---|---|
| `AruaruDb.connect(dsn)` / `AruaruDbSync.connect(dsn)` | asyncpg / psycopg をそのまま |
| `.commit(message) -> str` | `SELECT aruaru_commit($1)` → commit_id |
| `.query_as_of(base_select, commit_id, *params)` | `base_select` 末尾へ ` AS OF COMMIT '<id>'` を安全付与。`is_safe_commit_id`(英数字+`-``_`、≤128)で検証、非安全は `InvalidCommitId`(ネットワーク前に弾く) |
| `.query_as_of_val(...)` | 単一値版 |
| `.raw` | 内部の asyncpg Pool / psycopg Connection(透過) |

## FastAPI(非同期)

```python
from contextlib import asynccontextmanager
from fastapi import FastAPI
from aruaru_db import AruaruDb

@asynccontextmanager
async def lifespan(app):
    app.state.db = await AruaruDb.connect("postgresql://app:secret@localhost:5433/app")
    yield
    await app.state.db.raw.close()

app = FastAPI(lifespan=lifespan)

@app.post("/items/{item_id}")
async def upsert(item_id: str, qty: int):
    db = app.state.db
    await db.execute("INSERT INTO items(id,qty) VALUES ($1,$2) "
                     "ON CONFLICT (id) DO UPDATE SET qty=EXCLUDED.qty", item_id, qty)
    return {"commit_id": await db.commit("api write")}

@app.get("/items/{item_id}/at/{commit_id}")
async def as_of(item_id: str, commit_id: str):
    qty = await app.state.db.query_as_of_val(
        "SELECT qty FROM items WHERE id = $1", commit_id, item_id)
    return {"qty": qty}
```

## Django(同期)

`settings.py` は Django 標準の `postgresql` バックエンド(`PORT: '5433'`)を
そのまま。Git-on-SQL だけこのラッパーで:

```python
from aruaru_db import AruaruDbSync

db = AruaruDbSync.connect("host=localhost port=5433 dbname=app user=app password=secret")
commit = db.commit("nightly snapshot")
old = db.query_as_of_val("SELECT qty FROM items WHERE id = %s", commit, ("sword",))
```
(Django ORM の通常 CRUD はそのまま `django.db.backends.postgresql` で。)

## Flask + SQLAlchemy(同期)

`create_engine("postgresql+psycopg://app:secret@localhost:5433/app")` は
標準どおり。`aruaru_commit` / `AS OF COMMIT` は
`AruaruDbSync.from_connection(engine.raw_connection().driver_connection)` か
`db.session.execute(text("SELECT aruaru_commit(:m)"), {"m": msg})` で。

## テスト

```bash
python -m unittest discover -s tests   # ネットワーク不要(4 件)= green
```
実サーバ往復は別途 `asyncpg` / `psycopg` を入れて DSN を通す。

## 検証状況(2026-09-03、asyncpg 実サーバ往復)

- `pip install asyncpg`(0.31.0)を実際に導入し、`live-check.py`
  (`ARUARU_DB_DSN=... python live-check.py`)でローカル `aruaru-server`
  (:5433)への実往復を試みたが、**`asyncpg.connect()`/
  `asyncpg.create_pool()` の時点でハング**(30秒タイムアウトで強制終了、
  `pool=False` の単一接続でも同様)し、`commit()`/`query_as_of()` まで
  到達しなかった。**未解決**。
- 推定原因(未確証): asyncpg は接続確立時に PostgreSQL の型カタログへ
  イントロスペクションクエリを発行する(内蔵の `introspection.py`)。
  同時期に `.NET`(Npgsql)コネクタでも同種の症状
  (`NpgsqlDataSource` の既定ブートストラップが `pg_catalog` の完全
  実装を前提とし `"SELECT: missing FROM"` で失敗)が実際に発生し、
  `ServerCompatibilityMode.NoTypeLoading` で回避できた(`../dotnet-
  aruaru-db/README.md` 参照)。asyncpg 側でも同様の型イントロスペクション
  回避オプション(`server_settings`・カスタム型コーデック登録による
  ブートストラップ短絡等)が無いか、次回調査すること。
- Rust `tokio-postgres`(`rust-aruaru-db`)/ Node `pg`(`node-aruaru-db`)/
  .NET `Npgsql`(`NoTypeLoading` 設定後)はいずれも実サーバ往復まで
  green 済み(2026-09-03)——asyncpg 固有の問題である可能性が高い。
