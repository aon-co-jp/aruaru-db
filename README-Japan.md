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

## インストール(v0.1.0〜、Linux/Windows)

[GitHub Releases](https://github.com/aon-co-jp/aruaru-db/releases)から
`aruaru-server`バイナリ入りのtar.gz(Linux)/zip(Windows)をダウンロード
し、同梱の`install.sh`(Linux、systemdサービス登録)/`install.ps1`
(Windows、要管理者権限)を実行してください。詳細は
[install.sh](install.sh)/[install.ps1](install.ps1)冒頭のコメント参照。

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
| `aruaru-dist` | openraft 統合・Range シャーディング・ノード管理・Raftコミット×open-raid-zスナップショット連携(`snapshot_pairing`、2026-07-13追加) |
| `aruaru-query` | SQL パーサ・HTAP ルーター・DataFusion 統合 |
| `aruaru-wire` | PostgreSQL ワイヤプロトコル (pgwire) |
| `aruaru-graphql` | Versionless GraphQL + Poem HTTP サーバ |
| `aruaru-registry` | 対応DBレジストリ (150+件) + 毎日クロール + 取り込みアダプタ |
| `aruaru-migrate` | Postgres / CockroachDB / Snowflake / MySQL / CSV 移行ツール |
| `aruaru-backup` | バックアップ・リストア・ポイントインタイムリカバリ (Parquet) |
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

> **新機能 (2026-07-13)**: `SELECT col FROM t WHERE pk = 'v' AS OF COMMIT
> '<commit_id>'` 構文に対応。PKで特定した行の、最新値ではなく**指定した
> 過去コミット時点の値**を返す(単一行のみ対応、全表スキャンは未対応。
> pgwire経由での外部呼び出しへの配線はまだ未実施——詳細は正本の
> `README.md`「🌿 Git-on-SQL の使い方」節、および本ファイル下部の
> CLAUDE.md HANDOFF相当の記載参照)。

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
