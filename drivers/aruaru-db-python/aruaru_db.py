"""
aruaru-DB Python ドライバー

パッケージ名: aruaru-db-python
pip install aruaru-db-python
内部依存: asyncpg>=0.30, psycopg[binary]>=3.2

aruaru-DB は PostgreSQL ワイヤ互換のため、asyncpg / psycopg3 がそのまま使えます。
このドライバーは Git-on-SQL 操作を型付き API で包むラッパーです。
"""

from __future__ import annotations
from dataclasses import dataclass
from typing import Any, Optional

# ── 型定義 ────────────────────────────────────────────────────

@dataclass
class CommitEntry:
    id:        str
    short_id:  str
    author:    str
    message:   str
    timestamp: str
    root_hash: str

@dataclass
class DiffStat:
    added:    int
    removed:  int
    modified: int

# ── asyncpg 非同期ドライバー (推奨) ──────────────────────────

import asyncpg
import asyncio


class AruaruDBDriver:
    """
    aruaru-DB Python ドライバー (asyncpg ベース・非同期)

    Example::

        async def main():
            db = await AruaruDBDriver.connect("aruaru://root@localhost:5432/aruaru")
            await db.branch("feature/py-test")
            await db.execute("INSERT INTO tasks (id, title) VALUES ($1, $2)", 1, "Hello")
            commit_id = await db.commit("Add tasks via aruaru-db-python")
            print("Committed:", commit_id)
            await db.close()
    """

    def __init__(self, conn: asyncpg.Connection) -> None:
        self._conn = conn

    # ── コンストラクタ ────────────────────────────────────────

    @classmethod
    async def connect(
        cls,
        url: str = "postgres://root@localhost:5432/aruaru",
    ) -> "AruaruDBDriver":
        """URL から接続する。"""
        conn = await asyncpg.connect(url)
        return cls(conn)

    @classmethod
    async def connect_params(
        cls,
        host: str = "localhost",
        port: int = 5432,
        database: str = "aruaru",
        user: str = "root",
        password: str = "",
    ) -> "AruaruDBDriver":
        conn = await asyncpg.connect(
            host=host, port=port, database=database, user=user, password=password
        )
        return cls(conn)

    # ── Git-on-SQL ───────────────────────────────────────────

    async def branch(self, name: str) -> None:
        """ブランチを作成する。"""
        await self._conn.execute("SELECT aruaru_branch($1)", name)

    async def checkout(self, name: str) -> None:
        """ブランチを切り替える。"""
        await self._conn.execute("SELECT aruaru_checkout($1)", name)

    async def current_branch(self) -> str:
        """現在のブランチ名を返す。"""
        return await self._conn.fetchval("SELECT aruaru_current_branch()")

    async def commit(self, message: str) -> str:
        """コミットしてコミット ID を返す。"""
        return await self._conn.fetchval("SELECT aruaru_commit($1)", message)

    async def merge(self, from_branch: str) -> str:
        """fast-forward マージしてコミット ID を返す。"""
        return await self._conn.fetchval("SELECT aruaru_merge($1)", from_branch)

    async def log(self, limit: int = 20) -> list[CommitEntry]:
        """コミットログを取得する。"""
        rows = await self._conn.fetch(
            "SELECT * FROM aruaru_log ORDER BY timestamp DESC LIMIT $1", limit
        )
        return [
            CommitEntry(
                id=r["id"], short_id=r["short_id"], author=r["author"],
                message=r["message"], timestamp=str(r["timestamp"]),
                root_hash=r.get("root_hash", ""),
            )
            for r in rows
        ]

    async def diff(self, from_branch: str, to_branch: str) -> DiffStat:
        """2ブランチ間の差分統計を返す。"""
        row = await self._conn.fetchrow(
            "SELECT * FROM aruaru_diff($1, $2)", from_branch, to_branch
        )
        return DiffStat(
            added=row.get("added", 0),
            removed=row.get("removed", 0),
            modified=row.get("modified", 0),
        )

    # ── 汎用 SQL ─────────────────────────────────────────────

    async def execute(self, sql: str, *params: Any) -> str:
        """SQL を実行して status 文字列を返す。"""
        return await self._conn.execute(sql, *params)

    async def fetch(self, sql: str, *params: Any) -> list[dict]:
        """SELECT を実行して行のリストを返す。"""
        rows = await self._conn.fetch(sql, *params)
        return [dict(r) for r in rows]

    async def fetchval(self, sql: str, *params: Any) -> Any:
        """SELECT の最初のセルを返す。"""
        return await self._conn.fetchval(sql, *params)

    async def close(self) -> None:
        await self._conn.close()

    async def __aenter__(self) -> "AruaruDBDriver":
        return self

    async def __aexit__(self, *_: Any) -> None:
        await self.close()


# ── 同期ラッパー (psycopg3) ───────────────────────────────────

import psycopg


class AruaruDBDriverSync:
    """
    aruaru-DB Python ドライバー (psycopg3 ベース・同期)

    Example::

        with AruaruDBDriverSync.connect() as db:
            db.branch("feature/sync-test")
            db.execute("INSERT INTO tasks VALUES (%s, %s)", (1, "Hello"))
            print(db.commit("Add tasks"))
    """

    def __init__(self, conn: psycopg.Connection) -> None:
        self._conn = conn

    @classmethod
    def connect(cls, url: str = "host=localhost port=5432 dbname=aruaru user=root") -> "AruaruDBDriverSync":
        return cls(psycopg.connect(url))

    def branch(self, name: str) -> None:
        self._conn.execute("SELECT aruaru_branch(%s)", (name,))

    def checkout(self, name: str) -> None:
        self._conn.execute("SELECT aruaru_checkout(%s)", (name,))

    def commit(self, message: str) -> Optional[str]:
        cur = self._conn.execute("SELECT aruaru_commit(%s)", (message,))
        row = cur.fetchone()
        return row[0] if row else None

    def log(self, limit: int = 20) -> list[dict]:
        cur = self._conn.execute(
            "SELECT * FROM aruaru_log ORDER BY timestamp DESC LIMIT %s", (limit,)
        )
        cols = [d[0] for d in cur.description or []]
        return [dict(zip(cols, row)) for row in cur.fetchall()]

    def execute(self, sql: str, params: tuple = ()) -> None:
        self._conn.execute(sql, params)

    def close(self) -> None:
        self._conn.close()

    def __enter__(self) -> "AruaruDBDriverSync":
        return self

    def __exit__(self, *_: Any) -> None:
        self.close()


# ── 使用例 ───────────────────────────────────────────────────
if __name__ == "__main__":
    async def main() -> None:
        async with await AruaruDBDriver.connect() as db:
            await db.branch("feature/python-test")
            await db.execute(
                "CREATE TABLE IF NOT EXISTS tasks (id INT, title TEXT)"
            )
            await db.execute("INSERT INTO tasks VALUES ($1, $2)", 1, "Hello")
            commit_id = await db.commit("Add tasks via aruaru-db-python")
            print("Committed:", commit_id)
            for e in await db.log(5):
                print(f"{e.short_id}  {e.author}: {e.message}")

    asyncio.run(main())
