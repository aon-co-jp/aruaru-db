"""aruaru-db + FastAPI + asyncpg (async) — minimal connector example.

正本: ../../docs/CLIENTS.md
aruaru-db 側に独自ドライバは不要。標準の asyncpg でそのまま繋がる。
同期が必要なら psycopg 3 の sync API へ差し替えるだけ(ワイヤは同一)。

Run:
    pip install fastapi uvicorn asyncpg
    export ARUARU_DB_DSN="postgresql://app:secret@localhost:5433/app?ssl=require"
    uvicorn app:app --port 8000
"""

import os
import asyncpg
from contextlib import asynccontextmanager
from fastapi import FastAPI

DSN = os.environ.get("ARUARU_DB_DSN", "postgresql://app:secret@localhost:5433/app")


@asynccontextmanager
async def lifespan(app: FastAPI):
    app.state.pool = await asyncpg.create_pool(DSN, min_size=1, max_size=8)
    async with app.state.pool.acquire() as c:
        await c.execute(
            "CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY, qty INT)"
        )
    yield
    await app.state.pool.close()


app = FastAPI(lifespan=lifespan)


@app.post("/items/{item_id}")
async def upsert_and_commit(item_id: str, qty: int, message: str = "api write"):
    """Write a row, take a Git-on-SQL commit, return the commit id."""
    async with app.state.pool.acquire() as c:
        async with c.transaction():
            await c.execute(
                "INSERT INTO items (id, qty) VALUES ($1, $2) "
                "ON CONFLICT (id) DO UPDATE SET qty = EXCLUDED.qty",
                item_id,
                qty,
            )
        commit_id = await c.fetchval("SELECT aruaru_commit($1)", message)
    return {"id": item_id, "qty": qty, "commit_id": commit_id}


@app.get("/items/{item_id}")
async def get_latest(item_id: str):
    async with app.state.pool.acquire() as c:
        qty = await c.fetchval("SELECT qty FROM items WHERE id = $1", item_id)
    return {"id": item_id, "qty": qty}


@app.get("/items/{item_id}/at/{commit_id}")
async def get_as_of(item_id: str, commit_id: str):
    """VersionlessAPI: read the value as of a past commit."""
    async with app.state.pool.acquire() as c:
        qty = await c.fetchval(
            "SELECT qty FROM items WHERE id = $1 AS OF COMMIT $2",
            item_id,
            commit_id,
        )
    return {"id": item_id, "as_of": commit_id, "qty": qty}
