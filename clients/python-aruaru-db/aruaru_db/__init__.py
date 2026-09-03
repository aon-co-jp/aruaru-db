"""aruaru-db 公式 Python コネクタ(薄いラッパー) / official Python connector.

**これは独自の PostgreSQL ドライバではない。** 標準の `asyncpg`(非同期)/
`psycopg` v3(同期)をそのまま使い、その上に aruaru-db の Git-on-SQL 機能
——`SELECT aruaru_commit('msg')` と `... AS OF COMMIT '<id>'`——を Python の
慣用 API で足しただけの薄い層。

- 非同期(FastAPI 等): ``AruaruDb``  (要 ``asyncpg``)
- 同期(Django / Flask 等): ``AruaruDbSync``  (要 ``psycopg[binary]``)

正本: ../../docs/CLIENTS.md

依存は import 時ではなく ``connect`` 時に読むので、``is_safe_commit_id`` 等の
ネットワーク不要な関数はドライバ未インストールでも使える。
"""

from __future__ import annotations

import re
from typing import Any, Optional, Sequence

__all__ = ["AruaruDb", "AruaruDbSync", "is_safe_commit_id", "InvalidCommitId"]

_SAFE_COMMIT_ID = re.compile(r"\A[A-Za-z0-9_-]{1,128}\Z")


class InvalidCommitId(ValueError):
    """`query_as_of` に渡された commit_id がリテラルとして安全でない。"""


def is_safe_commit_id(commit_id: str) -> bool:
    """commit_id が ``AS OF COMMIT '<id>'`` のリテラルとして安全か。

    aruaru-db の commit_id は英数字 + ``-`` ``_``(SHA 系ハッシュ / UUID 由来)。
    それ以外(空白・引用符・セミコロン等)は SQL インジェクションの恐れが
    あるため弾く。
    """
    return bool(_SAFE_COMMIT_ID.match(commit_id or ""))


def _as_of_sql(base_select: str, commit_id: str) -> str:
    if not is_safe_commit_id(commit_id):
        raise InvalidCommitId(
            f"commit id {commit_id!r} is not a safe literal "
            f"(expected hex / [A-Za-z0-9_-], <=128 chars)"
        )
    return f"{base_select.rstrip()} AS OF COMMIT '{commit_id}'"


class AruaruDb:
    """非同期(``asyncpg``)コネクタ。FastAPI などの async アプリ向け。

    Example::

        db = await AruaruDb.connect("postgresql://app:secret@localhost:5433/app")
        await db.execute("INSERT INTO items(id, qty) VALUES ('sword', 1)")
        commit = await db.commit("first import")
        rows = await db.query_as_of("SELECT qty FROM items WHERE id = $1",
                                    commit, "sword")
    """

    def __init__(self, pool_or_conn: Any) -> None:
        # asyncpg の Pool でも Connection でも同じ execute/fetch API。
        self._c = pool_or_conn

    @classmethod
    async def connect(cls, dsn: str, *, pool: bool = True, **kw: Any) -> "AruaruDb":
        import asyncpg  # noqa: WPS433  (lazy: driver only needed here)

        if pool:
            obj = await asyncpg.create_pool(dsn, **kw)
        else:
            obj = await asyncpg.connect(dsn, **kw)
        return cls(obj)

    @classmethod
    def from_pool(cls, pool: Any) -> "AruaruDb":
        """既存の ``asyncpg`` Pool / Connection を包む。"""
        return cls(pool)

    @property
    def raw(self) -> Any:
        """内部の asyncpg Pool / Connection。透過的に何でも。"""
        return self._c

    async def execute(self, sql: str, *params: Any) -> str:
        return await self._c.execute(sql, *params)

    async def fetch(self, sql: str, *params: Any) -> list:
        return await self._c.fetch(sql, *params)

    async def fetchval(self, sql: str, *params: Any) -> Any:
        return await self._c.fetchval(sql, *params)

    async def commit(self, message: str) -> str:
        """Git-on-SQL: 全テーブルをスナップショットし commit_id を返す。"""
        cid = await self._c.fetchval("SELECT aruaru_commit($1)", message)
        if cid is None:
            raise RuntimeError("aruaru_commit() returned no commit id")
        return str(cid)

    async def query_as_of(
        self, base_select: str, commit_id: str, *params: Any
    ) -> list:
        """VersionlessAPI: ``base_select`` を過去のコミット時点で読む。

        ``base_select`` は ``AS OF COMMIT`` を**含まない**通常の SELECT。
        """
        return await self._c.fetch(_as_of_sql(base_select, commit_id), *params)

    async def query_as_of_val(
        self, base_select: str, commit_id: str, *params: Any
    ) -> Any:
        return await self._c.fetchval(_as_of_sql(base_select, commit_id), *params)


class AruaruDbSync:
    """同期(``psycopg`` v3)コネクタ。Django / Flask などの sync アプリ向け。

    Example::

        db = AruaruDbSync.connect("host=localhost port=5433 dbname=app user=app password=secret")
        db.execute("INSERT INTO items(id, qty) VALUES (%s, %s)", ("sword", 1))
        commit = db.commit("first import")
        rows = db.query_as_of("SELECT qty FROM items WHERE id = %s", commit, ("sword",))
    """

    def __init__(self, conn: Any) -> None:
        self._conn = conn

    @classmethod
    def connect(cls, dsn: str, **kw: Any) -> "AruaruDbSync":
        import psycopg  # noqa: WPS433  (lazy)

        return cls(psycopg.connect(dsn, autocommit=True, **kw))

    @classmethod
    def from_connection(cls, conn: Any) -> "AruaruDbSync":
        return cls(conn)

    @property
    def raw(self) -> Any:
        return self._conn

    def execute(self, sql: str, params: Optional[Sequence[Any]] = None) -> None:
        with self._conn.cursor() as cur:
            cur.execute(sql, params or ())

    def fetchall(self, sql: str, params: Optional[Sequence[Any]] = None) -> list:
        with self._conn.cursor() as cur:
            cur.execute(sql, params or ())
            return cur.fetchall()

    def fetchval(self, sql: str, params: Optional[Sequence[Any]] = None) -> Any:
        with self._conn.cursor() as cur:
            cur.execute(sql, params or ())
            row = cur.fetchone()
            return row[0] if row else None

    def commit(self, message: str) -> str:
        cid = self.fetchval("SELECT aruaru_commit(%s)", (message,))
        if cid is None:
            raise RuntimeError("aruaru_commit() returned no commit id")
        return str(cid)

    def query_as_of(
        self, base_select: str, commit_id: str, params: Optional[Sequence[Any]] = None
    ) -> list:
        return self.fetchall(_as_of_sql(base_select, commit_id), params)

    def query_as_of_val(
        self, base_select: str, commit_id: str, params: Optional[Sequence[Any]] = None
    ) -> Any:
        return self.fetchval(_as_of_sql(base_select, commit_id), params)
