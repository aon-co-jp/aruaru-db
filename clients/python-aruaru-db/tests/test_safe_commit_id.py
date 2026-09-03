"""ネットワーク不要のテスト。`python -m unittest` で走る(pytest でも可)。"""

import sys
import os
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from aruaru_db import is_safe_commit_id, InvalidCommitId  # noqa: E402
from aruaru_db import _as_of_sql  # noqa: E402


class SafeCommitId(unittest.TestCase):
    def test_accepts_hashes_and_uuids(self):
        self.assertTrue(is_safe_commit_id("a1b2c3d4e5f6"))
        self.assertTrue(is_safe_commit_id("9f8e7d6c-1234-4abc-9def-000011112222"))
        self.assertTrue(is_safe_commit_id("commit_42-X"))

    def test_rejects_sql_and_whitespace_and_empty(self):
        self.assertFalse(is_safe_commit_id(""))
        self.assertFalse(is_safe_commit_id("abc'; DROP TABLE items; --"))
        self.assertFalse(is_safe_commit_id("abc def"))
        self.assertFalse(is_safe_commit_id("x" * 200))
        self.assertFalse(is_safe_commit_id("' OR 1=1 --"))

    def test_as_of_sql_appends_clause_for_safe_id(self):
        self.assertEqual(
            _as_of_sql("SELECT qty FROM items WHERE id = $1", "abc123"),
            "SELECT qty FROM items WHERE id = $1 AS OF COMMIT 'abc123'",
        )

    def test_as_of_sql_raises_before_network_for_unsafe_id(self):
        with self.assertRaises(InvalidCommitId):
            _as_of_sql("SELECT 1", "'; DROP TABLE x; --")


if __name__ == "__main__":
    unittest.main()
