"""実サーバ相手のライブ往復チェック(asyncpg)。CI等では実行しない。

Usage:
    ARUARU_DB_DSN="postgresql://app:secret@127.0.0.1:5433/aruaru" python live-check.py
"""
import asyncio
import os
import sys

from aruaru_db import AruaruDb


async def main() -> None:
    dsn = os.environ.get("ARUARU_DB_DSN")
    if not dsn:
        print("ARUARU_DB_DSN not set, skipping live check", file=sys.stderr)
        return
    db = await AruaruDb.connect(dsn, pool=False)
    try:
        await db.execute("CREATE TABLE IF NOT EXISTS py_items (id TEXT, qty TEXT)")
        await db.execute("INSERT INTO py_items (id, qty) VALUES ($1, $2)", "sword", "1")
        commit1 = await db.commit("py first import")
        await db.execute("UPDATE py_items SET qty = $1 WHERE id = $2", "5", "sword")
        commit2 = await db.commit("py bump qty")

        as_of = await db.query_as_of_val(
            "SELECT qty FROM py_items WHERE id = $1", commit1, "sword"
        )
        latest = await db.query_as_of_val(
            "SELECT qty FROM py_items WHERE id = $1", commit2, "sword"
        )
        print(f"as-of qty={as_of!r} / latest qty={latest!r}")
        assert as_of == "1", f"expected as-of qty=1, got {as_of!r}"
        assert latest == "5", f"expected latest qty=5, got {latest!r}"
        print("OK")
    finally:
        await db.raw.close()


if __name__ == "__main__":
    asyncio.run(main())
