# PORTING.md — aruaru-DB お引越しファイル

> このファイル 1 枚で、**どのプロジェクトでも aruaru-DB を導入・移設**できます。
> 新しいバックエンド/データ層リポジトリにこのファイルをコピーして、
> 上から順に進めてください。
>
> 対象バージョン: workspace 0.5.0(9 クレート / 76 テスト、2026-07-11 実測。
> 最新のクレート数・テスト数は `CLAUDE.md` の「現状」節参照)
> 最終更新: 2026-07-11

---

## 0. aruaru-DB とは何か・エコシステム内での位置づけ

CockroachDB 的な分散強整合(openraft)と Snowflake 的なストレージ/コンピュート
分離(Arrow/DataFusion)を組み合わせつつ、**Git-on-SQL**(ブランチ・コミット・
diff・マージが SQL 関数として使える)というユニークな機能を持つ Pure Rust の
分散データベース。PostgreSQL ワイヤプロトコル(pgwire)・Versionless GraphQL・
Tauri 管理 GUI を備える。

`open-web-server` を中心に `poem-cosmo-tauri`/`open-runo`・PostgreSQL・
`open-raid-z` と組み合わせる目標アーキテクチャ(3D オンラインゲームの課金
アイテム・金融/証券データを紛失させないための TCP-IP/UDP-IP 三重通信 +
VersionLess API と Git 管理のハイブリッド版管理)では、aruaru-DB は
その **分散 Git-on-SQL データ層**を担当する(詳細は `CLAUDE.md` 参照)。

## 1. aruaru-DB とは(30 秒版)

| 分類 | 提供機能 |
|------|----------|
| 分散強整合 | openraft(Raft)による Range シャーディング・ノード管理 |
| HTAP クエリ | OLTP サブセットエンジン + DataFusion (Arrow/Parquet) の OLAP 経路 |
| プロトコル | pgwire(PostgreSQL ワイヤ互換)・Versionless GraphQL(Poem/async-graphql) |
| Git-on-SQL | `aruaru_branch`/`aruaru_commit`/`aruaru_diff`/`aruaru_merge`/`aruaru_log` |
| 移行ツール | PostgreSQL / CockroachDB / Snowflake / MySQL / CSV → aruaru-DB |
| レジストリ | 対応 DB 150+件のレジストリ + 毎日クロール + 取り込みアダプタ |
| バックアップ | フルスナップショット・リストア・SHA-256 検証・MANIFEST.json(Parquet ベース) |
| 管理 GUI | Tauri + TypeScript(`admin/`、WASM 移行は未着手) |

## 2. 持っていくもの(ファイル一覧)

```
aruaru-db/
├── Cargo.toml / Cargo.lock   ← workspace 定義(バージョン固定)
├── crates/                   ← 9 クレート(本体)
│   ├── aruaru-core            ストレージエンジン・MVCC・Git-on-SQL バージョン管理
│   ├── aruaru-dist            openraft 統合・Range シャーディング
│   ├── aruaru-query           SQL パーサ・HTAP ルーター・DataFusion 統合
│   ├── aruaru-wire            PostgreSQL ワイヤプロトコル(pgwire)
│   ├── aruaru-graphql         Versionless GraphQL + Poem HTTP サーバ
│   ├── aruaru-registry        対応 DB レジストリ + 取り込みアダプタ
│   ├── aruaru-migrate         Postgres/CockroachDB/Snowflake/MySQL/CSV 移行
│   ├── aruaru-backup          バックアップ・リストア・PITR(Parquet)
│   └── aruaru-server          メインバイナリ(統合エントリポイント)
├── admin/                    ← Tauri + TypeScript 管理 GUI(任意)
├── docs/                     ← ARCHITECTURE.md / DATABASE.md ほか設計文書
├── docker/ Dockerfile ほか   ← コンテナ化
├── .github/workflows/        ← CI(fmt / clippy / test)
└── PORTING.md                ← 本ファイル
```

丸ごと移設する場合はフォルダごとコピーして `cargo test --workspace`
(76 テストが通れば移設成功、`admin/` を使う場合は別途 `npm install`)。
以下はライブラリとして一部だけ使う場合です。

## 3. 依存の書き方(新プロジェクトの Cargo.toml)

```toml
[dependencies]
# 同一マシンにある場合(path 依存)
aruaru-core     = { path = "../aruaru-db/crates/aruaru-core" }
aruaru-query    = { path = "../aruaru-db/crates/aruaru-query" }
aruaru-dist     = { path = "../aruaru-db/crates/aruaru-dist" }
aruaru-wire     = { path = "../aruaru-db/crates/aruaru-wire" }
aruaru-graphql  = { path = "../aruaru-db/crates/aruaru-graphql" }

# GitHub 公開後は git 依存でも可
# aruaru-core = { git = "https://github.com/aruaru-db/aruaru-db" }

tokio = { version = "1", features = ["full"] }
openraft = { version = "0.9", features = ["serde"] }
datafusion = "47"
```

`aruaru-graphql`/`aruaru-wire`/`aruaru-server` は現時点で `poem` /
`async-graphql-poem` / `pgwire` に直接依存している(エコシステム全体の
Tauri/Poem/Cosmo 非依存方針に未移行。`CLAUDE.md` 参照)。この 3 クレートを
使うと `poem` への依存が伝播する点に注意。`aruaru-core`/`aruaru-query`/
`aruaru-dist`/`aruaru-registry`/`aruaru-migrate`/`aruaru-backup` は
Poem に依存しないため、単体のライブラリとして持ち出しやすい。

## 4. 組み込みレシピ

### 4.1 フルスタック(pgwire + GraphQL + Git-on-SQL ぜんぶ)

`aruaru-server` の `main.rs` をそのまま流用するのが最速です:

```bash
cargo run -p aruaru-server -- --data ./data --raft-id 1
# psql -h localhost -U root -d aruaru
# open http://localhost:4000/graphql
```

### 4.2 Git-on-SQL だけを既存 SQL エンジンに追加したい場合

```rust
use aruaru_core::version_tree::{VersionTree, Branch};

let tree = VersionTree::open("./data")?;
tree.branch("feature/new-schema")?;
// ... テーブル変更 ...
tree.commit("feature/new-schema", "Add score column to users")?;
let diff = tree.diff("main", "feature/new-schema")?;
tree.merge("feature/new-schema", "main")?;
```

SQL 経由でも同等の操作が可能(`SELECT aruaru_branch(...)` 等、`README.md`
「🌿 Git-on-SQL の使い方」参照)。

### 4.3 既存 Postgres/MySQL/CSV からの移行

```rust
use aruaru_migrate::{from_postgres, from_mysql, from_csv};

from_postgres::run_migration(pg_conn_str, target_client, &tables)?;
```

CLI からも: `cargo run -p aruaru-migrate -- --source postgres://... --tables users,orders`。
Snowflake へのエクスポートは Parquet 経路を共有(`docs/DATABASE.md` 参照)。

### 4.4 バックアップ・復元

```rust
use aruaru_backup::BackupEngine;

let engine = BackupEngine::new(query_engine.clone(), "./backups")?;
engine.run_full()?;                 // フルスナップショット(Parquet + SHA-256)
engine.restore("2026-07-11T00-00Z")?;
```

現状はフルダンプ方式(CoW 差分保存は未実装、`CLAUDE.md` 参照)。S3/SFTP
宛先は未接続で、ローカルディスクのみ実装済み。

## 5. データのお引越し(既存環境から)

```bash
# 旧環境からのフルスナップショット
cargo run -p aruaru-backup --bin backup-cli -- export --dest ./backups

# 新環境への取り込み
cargo run -p aruaru-backup --bin backup-cli -- restore --path ./backups/<manifest>.json
```

異種エンジン間の変換は `aruaru-migrate`(`docs/DATABASE.md` 参照)。

## 6. 環境変数・起動オプション 全一覧

| 変数/オプション | 既定 | 説明 |
|------|------|------|
| `--data <dir>` | 必須 | データディレクトリ(fjall LSM + Parquet 列ストア) |
| `--raft-id <n>` | 必須 | クラスタ内ノード ID |
| `--bind` | `0.0.0.0:5432` | pgwire 待受アドレス |
| `--graphql-bind` | `0.0.0.0:4000` | GraphQL 待受アドレス |
| `ARUARU_LOG_LEVEL` | info | ログレベル |

正確な一覧は `aruaru-core::Config` および `aruaru-server --help` を参照
(バージョンにより増減するため、本ファイルには主要なもののみ記載)。

## 7. REST/SQL/GraphQL サーフェス早見表

| インターフェース | 用途 |
|------|------|
| `psql -h <host> -p 5432` | PostgreSQL ワイヤ互換(pgwire) |
| `POST /graphql`(`GET /graphql` で GraphiQL) | Versionless GraphQL |
| `SELECT aruaru_branch/commit/diff/merge/log(...)` | Git-on-SQL バージョン管理 |
| `cargo run -p aruaru-migrate` | 他エンジンからの移行 CLI |
| `cargo run -p aruaru-backup` | バックアップ・リストア CLI |
| `admin/`(Tauri) | 管理 GUI(クラスタ状態・スキーマ・バックアップ操作) |

移設先で pgwire/GraphQL のどちらかしか要らない場合は、対応するクレート
(`aruaru-wire` または `aruaru-graphql`)だけを依存に加えれば足りる
(いずれも `aruaru-core`/`aruaru-query` を共通基盤として利用)。
