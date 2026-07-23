# aruaru-DB 🦀

> **The hybrid distributed database that speaks Git.**  
> CockroachDB の分散強整合 × Snowflake のストレージ/コンピュート分離 × Git-on-SQL バージョン管理 ── すべてを Pure Rust で。

[![Version](https://img.shields.io/badge/version-0.5.0-orange.svg)](https://github.com/aruaru-db/aruaru-db/releases)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/aruaru-db/aruaru-db/actions/workflows/ci.yml/badge.svg)](https://github.com/aruaru-db/aruaru-db/actions)
[![Discord](https://img.shields.io/badge/Discord-community-5865F2.svg)](https://discord.gg/aruaru-db)

📖 他言語: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

---

## ✨ なぜ aruaru-DB か

| 機能 | CockroachDB | Snowflake | **aruaru-DB** |
|------|------------|-----------|---------------|
| 分散強整合 (Raft) | ✅ | ❌ | ✅ |
| ストレージ/コンピュート分離 | ❌ | ✅ | ✅ |
| 列指向 OLAP (Arrow/DataFusion) | ❌ | ✅ | ✅ |
| Git-on-SQL (branch / merge / diff) | ❌ | ❌ | ✅ |
| Versionless GraphQL API | ❌ | ❌ | ✅ |
| Tauri 管理 GUI | ❌ | ❌ | ✅ |
| 移行ツール (Postgres / MySQL / CSV) | △ | △ | ✅ |
| **完全 OSS (Apache-2.0)** | ❌ (2024〜) | ❌ | ✅ |
| Pure Rust | ❌ (Go) | ❌ | ✅ |

---

## 🏗️ アーキテクチャ概要

```
┌──────────────────────────────────────────────────────────┐
│  Layer 3 : Access                                        │
│  pgwire (PostgreSQL互換)  │  GraphQL (Poem/async-graphql)│
│  REST API                 │  Tauri Admin GUI             │
├──────────────────────────────────────────────────────────┤
│  Layer 2 : Query & Distribution                          │
│  HTAP Router  │  DataFusion (OLAP)  │  openraft (Raft)  │
│  MVCC         │  Range Sharding     │  SQL Planner       │
├──────────────────────────────────────────────────────────┤
│  Layer 1 : Storage                                       │
│  Row Store (fjall LSM)  │  Columnar (Arrow / Parquet)   │
│  Version Tree (Prolly)  │  WAL (Write-Ahead Log)        │
└──────────────────────────────────────────────────────────┘
```

詳細は [ARCHITECTURE.md](ARCHITECTURE.md) と [docs/DATABASE.md](docs/DATABASE.md) を参照。

---

## 🚀 クイックスタート

```bash
# サーバ起動 (PostgreSQL ポート 5432 + GraphQL :4000)
cargo run -p aruaru-server -- --data ./data --raft-id 1

# psql で接続
psql -h localhost -U root -d aruaru

# GraphQL エンドポイント
open http://localhost:4000/graphql
```

### Tauri Admin GUI

```bash
cd admin
npm install
npm run tauri dev
```

---

## 📦 クレート構成

| クレート | 役割 |
|---------|------|
| `aruaru-core` | ストレージエンジン・MVCC・Git-on-SQL バージョン管理 |
| `aruaru-dist` | openraft 統合・Range シャーディング・ノード管理・Raftコミット×open-raid-zスナップショット連携(`snapshot_pairing`、2026-07-13追加)・Multi-Raft(CockroachDB/TiKV方式、Range単位の独立合意グループ、`multi_raft`、2026-07-23追加) |
| `aruaru-query` | SQL パーサ・HTAP ルーター(TiDB/TiFlash方式の行→列インクリメンタル同期`OlapCache`、2026-07-23追加)・DataFusion 統合 |
| `aruaru-wire` | PostgreSQL ワイヤプロトコル (pgwire) |
| `aruaru-graphql` | Versionless GraphQL + Poem HTTP サーバ |
| `aruaru-registry` | 対応DBレジストリ (150+件) + 毎日クロール + 取り込みアダプタ |
| `aruaru-migrate` | Postgres / CockroachDB / Snowflake / MySQL / CSV 移行ツール |
| `aruaru-backup` | バックアップ・リストア・ポイントインタイムリカバリ (Parquet) |
| `aruaru-server` | メインバイナリ (全クレートの統合エントリポイント) |

---

## 🌿 Git-on-SQL の使い方

> ⚠️ 以前の版に載っていた `ALTER TABLE` と `SELECT aruaru_diff(...)` は
> **現在のSQLパーサーには実装されていません**(コード確認済み、2026-07-12)。
> 以下は実際に動作する構文のみで置き換えたものです。

```sql
-- ブランチ作成 → 切り替え
SELECT aruaru_branch('feature/new-schema');
SELECT aruaru_checkout('feature/new-schema');

-- このブランチでデータ変更 (テーブル自体は事前に CREATE TABLE 済みとする)
INSERT INTO users (id, name, score) VALUES (1, 'Alice', 100);

-- コミット
SELECT aruaru_commit('Add score for Alice');

-- ログ確認
SELECT * FROM aruaru_log LIMIT 10;

-- main へ戻ってから feature をマージ (fast-forward)
-- 注意: aruaru_merge は引数を1つだけ取り、「現在のブランチ」に
-- 指定ブランチをマージする。旧版README にあった
-- aruaru_merge('feature/new-schema', 'main') という2引数呼び出しは
-- 実装(1引数のみ受け付ける)と一致しておらず、動作しません。
SELECT aruaru_checkout('main');
SELECT aruaru_merge('feature/new-schema');
```

### 過去コミット時点の状態を問い合わせる (`AS OF COMMIT`, 2026-07-13 追加)

VersionLessAPI(エンドポイントはバージョン番号を持たない)と Git 版管理
(データはコミット単位で完全な履歴を持つ)のハイブリッドの**読み出し側**。
`WHERE pk = 'value'` で行を1件特定できる場合、`AS OF COMMIT '<commit_id>'`
を付けると最新値ではなくその commit_id 時点の値を返します:

```sql
INSERT INTO items (id, qty) VALUES ('sword', 1);
SELECT aruaru_commit('first grant');          -- commit_id 例: abc123...

UPDATE items SET qty = '5' WHERE id = 'sword';
SELECT aruaru_commit('quantity bumped');

SELECT qty FROM items WHERE id = 'sword';                          -- 5 (最新)
SELECT qty FROM items WHERE id = 'sword' AS OF COMMIT 'abc123...'; -- 1 (過去)
```

内部では commit の `root_hash` から Prolly Tree を再構築して読み出すため、
最新の可変テーブル状態を経由しません。現状のスコープ: 単一行 (PK 一致の
`WHERE`) のみ対応、フルテーブルスキャンの `AS OF` は未対応(次回拡張候補)。
pgwire 経由(`open-runo`/`open-web-server` からの外部アクセス)にはまだ
配線されていません — 詳細は本ファイル下部の HANDOFF 節を参照。

ブランチ間の diff は SQL 関数としては提供されていません。`aruaru-graphql` の
GraphQL API 経由で取得します:

```graphql
query {
  diff(from: "main", to: "feature/new-schema") {
    added
    removed
    modified
  }
}
```

### UPSERT (2026-07-12 追加)

`ON CONFLICT ... DO UPDATE` / `DO NOTHING` に対応しています
(open-runo が生成するUPSERT文との互換性のために追加):

```sql
-- 初回は新規行としてINSERT、2回目以降(同じidが既にあれば)は
-- balance列だけをEXCLUDED(今回渡した新しい値)で上書き更新
INSERT INTO wallets (id, balance) VALUES (1, '500')
  ON CONFLICT (id) DO UPDATE SET balance = EXCLUDED.balance;

-- 既に存在する場合は何もしない (「無ければ作る」の冪等パターン)
INSERT INTO wallets (id, balance) VALUES (1, '500')
  ON CONFLICT (id) DO NOTHING;
```

> 現在の実装では、衝突判定はテーブルの**先頭列(=PK)**の重複でのみ行われます。
> `ON CONFLICT (col)` の `col` は先頭列と一致している必要があります(異なる列を
> 指定するとエラーになります)。

---

## 🔗 関連プロジェクト

`open-web-server` を中心に `poem-cosmo-tauri`/`open-runo`・PostgreSQL・
`open-raid-z` と組み合わせ、3Dオンラインゲームの課金アイテム・金融/証券
データを紛失させないための目標アーキテクチャがある(2026-07-11改訂:
通信層は TCP-IP・UDP-IP・QUIC/MPQUIC・MPTCP/SCTP の四重化、DB書き込みは
PostgreSQL・aruaru-db・マルチリージョン同期レプリケーション・独立監査ログ
の四重化)。aruaru-db はその分散 Git-on-SQL データ層として関与し、
VersionLessAPI とGit管理のハイブリッド版管理も担う。現状はTCP-IP・UDP-IP
のみ実装済みで他は未着手(詳細は `open-web-server` の `README.md`/
`CLAUDE.md` 参照)。

---

## 🤝 コントリビュート

世界中のボランティアによってメンテナンスされています。

- **Issues**: バグ報告・機能提案は GitHub Issues へ
- **good-first-issue** ラベルから始めてください
- `CONTRIBUTING.md` を必ずお読みください
- Discord: コミュニティチャンネルで議論
- 開発時、技術選定や仕様確認で迷ったら学習データの推測に頼らず、
  日本語・英語両方での検索や GitHub 調査で裏付けを取ることを推奨します

---

## 📄 ライセンス

Apache License 2.0 — 商用利用・改変・再配布すべて自由。  
© 2026 aruaru-DB Contributors
