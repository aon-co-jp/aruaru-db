# aruaru-db 公式 Mojo コネクタ(薄いラッパー) / official Mojo connector.
#
# **これは独自の PostgreSQL ドライバではない。** Mojo(Modular)は 2026 年
# 時点で成熟したネイティブ PostgreSQL ドライバを持たない(まだ若い
# システムプログラミング言語で、DB ドライバのエコシステムは Python ほど
# 育っていない)。その代わり Mojo は `from python import Python` による
# 強力な Python 相互運用(CPython 埋め込み)を持つ——これを使い、既存の
# 公式 Python コネクタ `clients/python-aruaru-db/`(それ自体が標準
# `asyncpg`/`psycopg` の薄いラッパー)を**そのまま呼び出す**。
#
# This is NOT a from-scratch wire-protocol implementation. As of this
# writing, Mojo has no mature native PostgreSQL driver of its own — it is
# still a young, evolving systems language and the DB-driver ecosystem
# hasn't caught up to Python's. What Mojo *does* have is strong Python
# interop (`from python import Python`, CPython embedding), so this
# connector wraps the existing official Python connector
# (`clients/python-aruaru-db/`, itself a thin wrapper over standard
# `asyncpg`/`psycopg`) via that interop layer — the same "standard driver,
# thin wrapper" philosophy used by every other connector in this repo
# (`clients/rust-aruaru-db/`, `clients/go-aruaru-db/`, etc.), just with an
# extra Python hop instead of a native PostgreSQL driver hop.
#
# セットアップ / Setup:
#   Mojo が埋め込む Python 環境に `python-aruaru-db` (このリポジトリの
#   `clients/python-aruaru-db/`) と、その依存(`psycopg[binary]`、同期
#   経路のみ使うなら `asyncpg` は不要)がインストールされている必要が
#   ある。詳細・検証状況は README.md を参照。
#
# 正本 / source of truth: ../../docs/CLIENTS.md

from python import Python
from python import PythonObject


# commit_id が `AS OF COMMIT '<id>'` のリテラルとして安全か、を Mojo 側で
# ネイティブに検証する(他コネクタ ── Rust `is_safe_commit_id` / Go
# `IsSafeCommitID` / Python `is_safe_commit_id` ── と同じルール: 英数字 +
# `-` `_`、1〜128 文字)。ここは Python 相互運用を経由しない ── 他の全
# コネクタが「ネットワークに触れる前に、まずネイティブにローカル検証する」
# のと同じ設計を踏襲する(SQL インジェクション防止の判定自体は、相手側
# ドライバやネットワークに一切依存させない)。
#
# Whether `id` is safe to interpolate literally into `AS OF COMMIT '<id>'`,
# checked natively in Mojo (same rule as every other connector in this
# repo: alnum + `-`/`_`, 1..=128 chars). This does NOT go through the
# Python interop layer — like every other connector here, the safety
# check itself happens natively, before any network I/O or driver call.
fn is_safe_commit_id(id: String) -> Bool:
    let n = len(id)
    if n == 0 or n > 128:
        return False
    for i in range(n):
        let c = id[i]
        let is_digit = c >= "0" and c <= "9"
        let is_upper = c >= "A" and c <= "Z"
        let is_lower = c >= "a" and c <= "z"
        let is_dash = c == "-"
        let is_underscore = c == "_"
        if not (is_digit or is_upper or is_lower or is_dash or is_underscore):
            return False
    return True


# InvalidCommitId: query_as_of に渡された commit_id が安全なリテラルでは
# なかった場合に送出する(Mojo にはまだ標準的な例外階層が固まっていない
# ため、呼び出し側は raise された Error のメッセージで判別する)。
#
# Raised by `query_as_of` when the given commit id is not a safe literal.
# Mojo's exception story is still evolving, so callers should match on the
# Error's message text.
fn _invalid_commit_id_message(id: String) -> String:
    return (
        "commit id '"
        + id
        + "' is not a safe literal (expected alnum / '-' / '_', <=128 chars)"
    )


# AruaruDb: Python コネクタ(`aruaru_db.AruaruDbSync`)への薄いラッパー。
# 内部で保持するのは Python オブジェクト(`aruaru_db.AruaruDbSync` の
# インスタンス)そのもの ── Mojo 側で独自にコネクション状態を再実装しない。
#
# 同期(`AruaruDbSync`、`psycopg` v3)経路を既定にしている。理由: Mojo の
# 非同期ランタイム(async/await)は 2026 年時点でもまだ発展途上であり、
# Python 側の `asyncio` イベントループと Mojo 側のタスクスケジューラを
# 相互運用でまたぐのは信頼性の面で分がある方(同期経路)を選ぶのが
# 誠実だと判断した。非同期が要る場合は `raw()` で得た Python オブジェクト
# 経由で `aruaru_db.AruaruDb`(asyncpg 版)を直接使うこともできる
# (README 参照)。
#
# Thin wrapper over the Python connector's `aruaru_db.AruaruDbSync`. What
# this struct holds internally *is* the Python object (an instance of
# `aruaru_db.AruaruDbSync`) — no connection state is reimplemented in
# Mojo. Defaults to the sync (`psycopg` v3) path: Mojo's own async/await
# story is still evolving as of this writing, and bridging Python's
# `asyncio` event loop with Mojo's task scheduler across the interop
# boundary is a reliability trade-off not worth making here. If async is
# needed, reach into the Python `aruaru_db.AruaruDb` (asyncpg) class
# directly via `raw()` (see README).
struct AruaruDb:
    var _py_db: PythonObject

    fn __init__(inout self, py_db: PythonObject):
        self._py_db = py_db

    # dsn: libpq 形式("host=... port=5433 dbname=... user=... password=...")
    # または "postgresql://user:pass@host:5433/db" のどちらでも可(psycopg
    # がそのまま解釈する)。
    #
    # dsn: either libpq key-value form or a "postgresql://" URL — passed
    # straight through to psycopg.
    @staticmethod
    fn connect(dsn: String) raises -> AruaruDb:
        let aruaru_db_mod = Python.import_module("aruaru_db")
        let py_db = aruaru_db_mod.AruaruDbSync.connect(dsn)
        return AruaruDb(py_db)

    # raw(): 内部の Python オブジェクト(`aruaru_db.AruaruDbSync` インスタンス)
    # をそのまま返す。このラッパーがカバーしない機能へ透過的にアクセスする
    # 用途(例: 非同期版 `aruaru_db.AruaruDb` を別途 import して使う、
    # psycopg 固有のオプションを直接叩く、等)。
    #
    # Returns the underlying Python object as-is, for anything this thin
    # wrapper doesn't cover.
    fn raw(self) -> PythonObject:
        return self._py_db

    fn execute(self, sql: String) raises:
        _ = self._py_db.execute(sql)

    # commit: 全テーブルをスナップショットし、新しい commit_id を返す
    # (`SELECT aruaru_commit('message')`)。
    #
    # commit: snapshots all tables and returns the new commit id.
    fn commit(self, message: String) raises -> String:
        let cid = self._py_db.commit(message)
        return String(cid)

    # query_as_of: VersionlessAPI ── base_select(`AS OF COMMIT` を含まない
    # 通常の SELECT)を、過去の commit_id 時点で読む。
    #
    # commit_id は Mojo 側でネイティブに `is_safe_commit_id` 検証してから
    # Python 層へ渡す ── ネットワークはおろか Python 相互運用へ入る前に、
    # 不正な commit_id を弾く(他コネクタと同じ「ネットワークに触れる前の
    # ローカル検証」設計)。
    #
    # query_as_of: reads `base_select` (a plain SELECT with no
    # `AS OF COMMIT` clause) as of a historical commit. commit_id is
    # validated *natively in Mojo* via `is_safe_commit_id` before it ever
    # crosses into the Python interop layer — matching every other
    # connector's "reject before touching the network" design.
    fn query_as_of(self, base_select: String, commit_id: String) raises -> PythonObject:
        if not is_safe_commit_id(commit_id):
            raise Error(_invalid_commit_id_message(commit_id))
        return self._py_db.query_as_of(base_select, commit_id)

    fn query_as_of_val(self, base_select: String, commit_id: String) raises -> PythonObject:
        if not is_safe_commit_id(commit_id):
            raise Error(_invalid_commit_id_message(commit_id))
        return self._py_db.query_as_of_val(base_select, commit_id)


fn main() raises:
    # モジュールとして import された時にも、`mojo run aruaru_db.mojo` で
    # 直接実行された時にも困らないよう、ネットワーク不要の自己診断だけ
    # 行う小さな `main`。実際の接続例は README.md を参照。
    #
    # A tiny network-free self-check `main`, useful when this file is run
    # directly with `mojo run aruaru_db.mojo`. See README.md for real
    # connection examples.
    if is_safe_commit_id("a1b2c3d4e5f6") and not is_safe_commit_id("'; DROP TABLE items; --"):
        print("aruaru_db.mojo: is_safe_commit_id self-check passed")
    else:
        print("aruaru_db.mojo: is_safe_commit_id self-check FAILED")
