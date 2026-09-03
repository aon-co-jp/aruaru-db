'use strict';
// ネットワーク不要のテスト。`node test.js`(または `npm test`)。
const assert = require('node:assert');
const { isSafeCommitId, InvalidCommitId, _asOfSql } = require('./index.js');

let n = 0;
const ok = (name, fn) => { fn(); n++; console.log('ok', name); };

ok('accepts hashes and uuids', () => {
  assert.strictEqual(isSafeCommitId('a1b2c3d4e5f6'), true);
  assert.strictEqual(isSafeCommitId('9f8e7d6c-1234-4abc-9def-000011112222'), true);
  assert.strictEqual(isSafeCommitId('commit_42-X'), true);
});

ok('rejects sql / whitespace / empty / overlong / non-string', () => {
  assert.strictEqual(isSafeCommitId(''), false);
  assert.strictEqual(isSafeCommitId("abc'; DROP TABLE items; --"), false);
  assert.strictEqual(isSafeCommitId('abc def'), false);
  assert.strictEqual(isSafeCommitId('x'.repeat(200)), false);
  assert.strictEqual(isSafeCommitId(42), false);
  assert.strictEqual(isSafeCommitId(null), false);
});

ok('asOfSql appends the clause for a safe id', () => {
  assert.strictEqual(
    _asOfSql('SELECT qty FROM items WHERE id = $1', 'abc123'),
    "SELECT qty FROM items WHERE id = $1 AS OF COMMIT 'abc123'",
  );
});

ok('asOfSql throws InvalidCommitId before any network call', () => {
  assert.throws(() => _asOfSql('SELECT 1', "'; DROP TABLE x; --"), InvalidCommitId);
});

console.log(`\n${n} passed`);
