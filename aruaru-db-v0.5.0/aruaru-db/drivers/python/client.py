# aruaru-py: Python Client for aruaru-DB
# pip install aruaru-py

# ─── インストール ───────────────────────────────────────────────
# pip install aruaru-py
# 内部依存: asyncpg>=0.30, psycopg[binary]>=3.2

# ─── 使用例: 同期 (psycopg3) ──────────────────────────────────
import psycopg

def example_sync():
    with psycopg.connect("host=localhost port=5432 dbname=aruaru user=root") as conn:
        with conn.cursor() as cur:
            # 通常の SQL
            cur.execute("CREATE TABLE IF NOT EXISTS users (id SERIAL PRIMARY KEY, name TEXT)")
            cur.execute("INSERT INTO users (name) VALUES (%s)", ("Alice",))

            # Git-on-SQL: ブランチ作成
            cur.execute("SELECT aruaru_branch('feature/add-users')")

            # コミット
            cur.execute("SELECT aruaru_commit('Add Alice to users')")

            # ログ確認
            cur.execute("SELECT * FROM aruaru_log LIMIT 5")
            for row in cur.fetchall():
                print(row)

            conn.commit()

# ─── 使用例: 非同期 (asyncpg) ────────────────────────────────
import asyncpg
import asyncio

async def example_async():
    conn = await asyncpg.connect(
        host="localhost", port=5432,
        database="aruaru", user="root"
    )

    # 通常のクエリ
    await conn.execute("INSERT INTO users (name) VALUES ($1)", "Bob")

    # Git-on-SQL
    await conn.execute("SELECT aruaru_commit('Add Bob')")
    log = await conn.fetch("SELECT * FROM aruaru_log LIMIT 10")
    for row in log:
        print(dict(row))

    await conn.close()

# ─── aruaru-py ラッパー API ──────────────────────────────────
# from aruaru import AruaruDB

# async def example_wrapper():
#     db = await AruaruDB.connect("aruaru://root@localhost:5432/aruaru")
#
#     # Git-on-SQL ラッパー
#     await db.branch.create("feature/new-schema")
#     await db.execute("ALTER TABLE users ADD COLUMN email TEXT")
#     commit = await db.commit(author="PHI", message="Add email column")
#     print(f"Committed: {commit.short_id}")
#
#     diff = await db.diff("main", "feature/new-schema")
#     print(f"Changed: +{diff.added} -{diff.removed} ~{diff.modified}")
#
#     log = await db.log(limit=10)
#     for c in log:
#         print(f"{c.short_id}  {c.author}: {c.message}")
#
#     await db.close()

if __name__ == "__main__":
    asyncio.run(example_async())
