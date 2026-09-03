'use strict';
// 実サーバ相手の往復チェック。`npm i pg` してから:
//   ARUARU_DB_DSN="postgres://app:secret@127.0.0.1:5433/aruaru" node live-check.js
const assert = require('node:assert');
const { AruaruDb } = require('./index.js');

(async () => {
  const dsn = process.env.ARUARU_DB_DSN || 'postgres://app:secret@127.0.0.1:5433/aruaru';
  const db = await AruaruDb.connect(dsn);
  await db.query('CREATE TABLE IF NOT EXISTS nlive (id TEXT PRIMARY KEY, qty INT)');
  await db.query("INSERT INTO nlive(id,qty) VALUES ('s',1) ON CONFLICT (id) DO UPDATE SET qty=EXCLUDED.qty");
  const first = await db.commit('first import');
  assert.match(first, /^[A-Za-z0-9_-]+$/);
  await db.query("UPDATE nlive SET qty=5 WHERE id='s'");
  await db.commit('restock');

  const latest = await db.query("SELECT qty FROM nlive WHERE id='s'");
  assert.strictEqual(String(latest.rows[0].qty), '5');

  const old = await db.queryAsOf("SELECT qty FROM nlive WHERE id='s'", first);
  assert.strictEqual(String(old.rows[0].qty), '1', 'AS OF COMMIT must return the historical value');

  console.log('node live-check: OK  (first=%s, latest qty=5, as-of qty=1)', first);
  await db.raw.end();
})().catch((e) => { console.error('node live-check FAILED:', e); process.exit(1); });
