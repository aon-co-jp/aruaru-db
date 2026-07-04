# aruaru-DB 🦀

> **Gitを話す、ハイブリッド分散データベース。**  
> CockroachDB の分散強整合 × Snowflake のストレージ/コンピュート分離 × Git-on-SQL バージョン管理 ── すべてを Pure Rust で。

[![Version](https://img.shields.io/badge/version-0.5.1-orange.svg)](https://github.com/aruaru-db/aruaru-db/releases)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/aruaru-db/aruaru-db/actions/workflows/ci.yml/badge.svg)](https://github.com/aruaru-db/aruaru-db/actions)
[![Discord](https://img.shields.io/badge/Discord-community-5865F2.svg)](https://discord.gg/aruaru-db)

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

### 3層防御通信 (`aruaru-wire`)

`aruaru-wire` は pgwire 実装であると同時に、TLS(第1層) + 相互認証(第2層) +
ペイロード暗号化(第3層) の3層防御通信も担う。**open-web-server** / **open-runo** も
同一方針の crate (`open-web-server-wire`) でこの3層を実装しており、
3プロジェクト間の通信は常にこの防御レベルで統一されている。

---

## 🔗 open-web-server / open-runo との連携

aruaru-DB は単体のデータベースとしても使えるが、3Dオンラインゲームの課金アイテムや
金融データを扱う構成では、以下のように3層で連携する。

```text
Client → open-web-server（入口・WAL先行書き込み）
       → open-runo（Federation Gateway・認証/監査の一元管理）
       → aruaru-DB（Raft分散合意コミット・commit_id発行）
```

書き込みは `commit_id` が発行されるまで確定とみなさないため、途中の瞬断や
リトライがあっても二重書き込みは起きない。詳細は open-web-server リポジトリの
`docs/integration.md` を参照。

---

## 🧬 ARU3 三重書き込みストレージ (Layer 3 詳細設計)

「課金アイテムも、残高も、書き込んだ瞬間から二度と消えない」— この約束を、独自フォーマット
**ARU3** による追記専用(append-only)エンジンとして実装する。詳細仕様は
[`docs/FORMAT-ARU3-Japan.md`](docs/FORMAT-ARU3-Japan.md) を参照。

```
open-web-server (Layer 1) → open-runo (Layer 2) → aruaru-db (Layer 3) ×3レプリカ
```

aruaru-db は open-runo からのみ書き込まれる想定の、単純な「書く・読む」エンジン。
輻輳制御・リトライ・優先度レーンの判断はすべて Layer 2 (open-runo) が担い、
aruaru-db は完全性(整合性)だけに集中する。

### 核心技術: 二重チェックサム

各レコードは2つの BLAKE3 チェックサムを持つ。

- `payload_checksum` — open-web-server が計算したエンドツーエンドの値。そのまま保存。
- `record_checksum` — aruaru-db が書き込み直前に計算するローカルな値。

読み取り時に両方を検証することで、**「ネット経路で壊れたのか」「ディスクで壊れたのか」を
切り分け**られる。この切り分けは修復(anti-entropy)の起点として使われる。

### 主な特長

- **append-only** — 上書きしない。障害は「途中で切れる(torn write)」形でしか起きないため、検出と復旧が単純。
- **自己検証レコード** — マジックナンバー走査でファイル先頭からでなくても任意位置から復元可能。
- **WAL + fsync境界** — `FINANCE` フラグ付きレコードは WAL fsync → セグメント fsync 完了までACKしない。GAME用途は group commit でスループットを優先。
- **多数決読み取り(Quorum Read)** — 3レプリカから読み、`payload_checksum` の一致多数を採用。金融残高照会は `require_full_quorum` で 3/3 一致を強制できる。
- **クラッシュリカバリ** — 起動時にセグメント末尾の torn record を自動切り詰め、WAL再生で復元。

### 境界設計: open-runo との責務分離

aruaru-db は `Record`(ARU3形式)という独自の内部表現を持つが、これは意図的。
`Commit`(ネットワーク越しの共有型)から `Record`(ディスク上の永続形式)への変換は
open-runo の責務として明確に分離されており、ストレージ層の内部形式を変更しても
Layer 1/クライアントに影響が波及しない。共有すべきもの(`aruaru-common`)と、
層ごとに閉じるべきもの(`Record`)を意識的に分けている。

```rust
use aruaru_db::TripleStore;
use std::path::Path;

let mut store = TripleStore::open([
    Path::new("f:/open-aruaru/aruaru-db/data/r0"),
    Path::new("e:/open-aruaru/aruaru-db/data/r1"),
    Path::new("d:/aruaru-db/data/r2"),
])?;

// 通常は open-runo から呼ばれるが、単体テストや組み込み用途では直接利用可能
```

- aruaru-db 自体は open-runo/open-web-server の proto を一切参照しない(疎結合)。契約点は
  `Record` 構造体の `payload_checksum` フィールドのみで、これが Layer 1 で計算された
  BLAKE3 値をそのまま運ぶ。
- `TripleStore::open` に渡す3ディレクトリは open-runo の `ARU3_R0/R1/R2` 環境変数と
  1対1で対応させる。

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
| `aruaru-dist` | openraft 統合・Range シャーディング・ノード管理 |
| `aruaru-query` | SQL パーサ・HTAP ルーター・DataFusion 統合 |
| `aruaru-wire` | PostgreSQL ワイヤプロトコル (pgwire) + 3層防御通信 |
| `aruaru-graphql` | Versionless GraphQL + Poem HTTP サーバ |
| `aruaru-migrate` | Postgres / CockroachDB / Snowflake / CSV 移行ツール |
| `aruaru-server` | メインバイナリ (全クレートの統合エントリポイント) |

---

## 🌿 Git-on-SQL の使い方

```sql
-- ブランチ作成
SELECT aruaru_branch('feature/new-schema');

-- 現在のブランチでテーブル変更
ALTER TABLE users ADD COLUMN score INT DEFAULT 0;

-- コミット
SELECT aruaru_commit('Add score column to users');

-- ログ確認
SELECT * FROM aruaru_log LIMIT 10;

-- diff
SELECT * FROM aruaru_diff('main', 'feature/new-schema');

-- マージ
SELECT aruaru_merge('feature/new-schema', 'main');
```

---

## 🤝 コントリビュート

世界中のボランティアによってメンテナンスされています。

- **Issues**: バグ報告・機能提案は GitHub Issues へ
- **good-first-issue** ラベルから始めてください
- `CONTRIBUTING.md` を必ずお読みください
- Discord: コミュニティチャンネルで議論

---

## 📄 ライセンス

Apache License 2.0 — 商用利用・改変・再配布すべて自由。  
© 2026 aruaru-DB Contributors
