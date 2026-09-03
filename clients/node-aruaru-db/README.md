# `@aruaru/db` — 公式 Node コネクタ(薄いラッパー)

**独自の PostgreSQL ドライバではない。** 標準の [`pg`](https://www.npmjs.com/package/pg)
(node-postgres)をそのまま使い、その上に Git-on-SQL を Node の慣用 API で
足すだけ。Express / Fastify / NestJS、どの FW でも同じ。JS は基本非同期。

正本: [`../../docs/CLIENTS.md`](../../docs/CLIENTS.md)

## インストール

```bash
npm i @aruaru/db pg
```

## API

| メソッド | 実体 |
|---|---|
| `AruaruDb.connect(configOrString)` / `AruaruDb.fromPool(pool)` | `pg` の Pool/Client をそのまま |
| `.query(sql, params)` | `pg` の薄い透過 |
| `.commit(message) -> Promise<string>` | `SELECT aruaru_commit($1)` → commit_id(結果列名に依存せず先頭列を取る) |
| `.queryAsOf(baseSelect, commitId, params)` | 末尾へ ` AS OF COMMIT '<id>'` を安全付与。`isSafeCommitId`(`[A-Za-z0-9_-]{1,128}`)で検証、非安全は `InvalidCommitId` |
| `.raw` | 内部の `pg` Pool/Client |

`.d.ts` 同梱。

## 例

```js
import { AruaruDb } from '@aruaru/db';

const db = await AruaruDb.connect('postgres://app:secret@localhost:5433/app');
await db.query("INSERT INTO items(id,qty) VALUES ('sword',1) ON CONFLICT (id) DO UPDATE SET qty=EXCLUDED.qty");
const first = await db.commit('first import');
await db.query("UPDATE items SET qty=5 WHERE id='sword'");
await db.commit('restock');
const old = await db.queryAsOf("SELECT qty FROM items WHERE id='sword'", first);
// String(old.rows[0].qty) === '1'   (最新は '5')
await db.raw.end();
```

Express/Fastify/NestJS は起動時に `AruaruDb.connect(...)` して共有するだけ。

## 検証状況

- **`node test.js`(ネットワーク不要 4 件)= green**。
- **`node live-check.js`(実サーバ往復)= 2026-09-03 green**: `npm i pg` 後、
  ローカル `aruaru-server`(:5433、`ARUARU_USERS=app:secret`)相手に
  `connect` → `commit()` → `queryAsOf()`(拡張プロトコル)が過去値 `1`
  (最新 `5`)を返すことを確認(`AS OF COMMIT` 列射影修正 `1566c0b` 込み)。
- **注意(`docs/CLIENTS.md §5.1`)**: 結果列は現状すべて `VARCHAR`(text)。
  数値は `String(row.qty)` 等で受けて `Number(...)` で変換する。
