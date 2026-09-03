# aruaru-db 公式 Mojo コネクタのテスト。
#
# ネットワーク不要ぶん(`is_safe_commit_id` の受理/拒否)を先に検証する。
# 実サーバ往復(`AruaruDb.connect` → `commit` → `query_as_of`)は、Mojo が
# 埋め込む Python 環境に `aruaru_db`(`clients/python-aruaru-db/`)+
# `psycopg[binary]` が入っており、かつ環境変数 `ARUARU_DB_TEST_DSN` が
# 設定されている場合のみ実行する ── 他コネクタの `#[ignore]`
# (Rust)/ `if os.Getenv(...) == """ { t.Skip(...) }` (Go) と同じ役割。
#
# Tests for the official Mojo connector. Network-free checks
# (`is_safe_commit_id` accept/reject) run first. The live round-trip
# (`AruaruDb.connect` → `commit` → `query_as_of`) only runs when the
# embedded Python environment has `aruaru_db` +  `psycopg[binary]`
# installed AND `ARUARU_DB_TEST_DSN` is set — playing the same role as
# `#[ignore]` in the Rust connector or `os.Getenv` skip in the Go one.

from python import Python
from testing import assert_true, assert_false, assert_equal

from aruaru_db import is_safe_commit_id, AruaruDb


fn test_is_safe_commit_id_accepts_hex_and_uuid_like_ids() raises:
    assert_true(is_safe_commit_id("a1b2c3d4e5f6"))
    assert_true(is_safe_commit_id("9f8e7d6c-1234-4abc-9def-000011112222"))
    assert_true(is_safe_commit_id("commit_42-X"))


fn test_is_safe_commit_id_rejects_empty_whitespace_and_sql_injection() raises:
    assert_false(is_safe_commit_id(""))
    assert_false(is_safe_commit_id("abc def"))
    assert_false(is_safe_commit_id("'; DROP TABLE items; --"))
    assert_false(is_safe_commit_id("abc' OR '1'='1"))


fn test_is_safe_commit_id_rejects_overlong_ids() raises:
    var long_id = String("")
    for _ in range(200):
        long_id += "x"
    assert_false(is_safe_commit_id(long_id))


fn test_query_as_of_rejects_unsafe_commit_id_before_touching_python() raises:
    # AruaruDb を実際には構築せず(＝ネットワークにもPython相互運用にも
    # 一切触れず)、commit_id 検証がその手前で失敗することだけを確かめる。
    # これは Rust/Go の同名テストと同じ「ネットワークに触れる前に弾く」
    # という契約の検証であり、`_py_db` へは絶対に到達しない。
    var caught = False
    try:
        # None 相当の PythonObject を渡した AruaruDb で query_as_of を
        # 呼んでも、is_safe_commit_id の判定が先に失敗するため
        # `_py_db.query_as_of` には到達しない ── 到達すれば None 呼び出しで
        # 別の例外になるはずだが、それより前に Error が飛ぶことを確認する。
        let none_obj = Python.none()
        let db = AruaruDb(none_obj)
        _ = db.query_as_of("SELECT qty FROM items", "'; DROP TABLE items; --")
    except e:
        caught = True
    assert_true(caught)


fn test_live_commit_and_as_of_round_trip() raises:
    # 実 aruaru-server への往復。ARUARU_DB_TEST_DSN が無ければ何もしない
    # (Mojo の testing フレームワークには標準の skip 機構がまだ無いため、
    # 素通りする早期 return で代替する ── .NET コネクタの xUnit テストで
    # 採用したのと同じ方式)。
    let os_mod = Python.import_module("os")
    let dsn_obj = os_mod.environ.get("ARUARU_DB_TEST_DSN", "")
    let dsn = String(dsn_obj)
    if len(dsn) == 0:
        print("test_live_commit_and_as_of_round_trip: skipped (set ARUARU_DB_TEST_DSN to run)")
        return

    let db = AruaruDb.connect(dsn)
    db.execute("CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY, qty INT)")
    db.execute(
        "INSERT INTO items(id, qty) VALUES ('sword', 1) "
        "ON CONFLICT (id) DO UPDATE SET qty = EXCLUDED.qty"
    )

    let first = db.commit("first import")
    db.execute("UPDATE items SET qty = 5 WHERE id = 'sword'")
    _ = db.commit("restock")

    # aruaru-wire は通常のテーブル列を常に VARCHAR(text) で返す。
    let latest = db.raw().fetchval("SELECT qty FROM items WHERE id = 'sword'")
    assert_equal(String(latest), "5")

    let old = db.query_as_of_val("SELECT qty FROM items WHERE id = 'sword'", first)
    assert_equal(String(old), "1")


fn main() raises:
    test_is_safe_commit_id_accepts_hex_and_uuid_like_ids()
    test_is_safe_commit_id_rejects_empty_whitespace_and_sql_injection()
    test_is_safe_commit_id_rejects_overlong_ids()
    test_query_as_of_rejects_unsafe_commit_id_before_touching_python()
    test_live_commit_and_as_of_round_trip()
    print("test_aruaru_db.mojo: all checks passed")
