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
`open-raid-z` と組み合わせる目標アーキテクチャ(2026-07-11改訂: 3D
オンラインゲームの課金アイテム・金融/証券データを紛失させないための
通信層四重化 [TCP-IP・UDP-IP・QUIC/MPQUIC・MPTCP/SCTP] + DB書き込み
四重化 [PostgreSQL・aruaru-db・マルチリージョン同期レプリケーション・
独立監査ログ] + VersionLess API と Git 管理のハイブリッド版管理。現状は
TCP-IP・UDP-IPのみ実装済みで他は未着手)では、aruaru-DB は
その **分散 Git-on-SQL データ層**を担当する(詳細は `open-web-server` の
`CLAUDE.md` 参照)。

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

**`aruaru-dist::snapshot_pairing`(2026-07-13追加)**: `RaftNode` の
commit+適用完了フック(`set_commit_hook`)、`SnapshotBackend` トレイト、
`SnapshotPairingRegistry`(commit_index → snapshot_id 対応記録)は
`open_raid_z` featureに依存しない(デフォルトで利用可能、テスト・開発用の
`InMemorySnapshotBackend` のみでも動作する)ため、他プロジェクトへ
そのまま移設しやすい。`open_raid_z` feature有効時のみコンパイルされる
`raid_z_backend::OpenRaidZSnapshotBackend` は `open-raid-z` リポジトリの
`open_runo_zfs_source/open_raid_z_core` へのpath依存(`../../../open-raid-z/...`
という相対パス)を前提とするため、移設先でも両リポジトリを同じ相対位置
関係でチェックアウトする必要がある点に注意。

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

### 4.2.1 過去コミット時点の値を問い合わせる (`AS OF COMMIT`, 2026-07-13追加)

`open-web-server` 拡張要件(1)「VersionLessAPIとGit版管理のハイブリッド」の
読み出し側。`aruaru-query::engine::QueryEngine` に
`SELECT col FROM t WHERE pk = 'v' AS OF COMMIT '<commit_id>'` 構文を追加した
(パーサー: `aruaru-query/src/parser.rs` の `Statement::SelectAsOf`、実行:
`engine.rs` の `select_as_of`)。内部では
`aruaru_core::version::VersionController::get_commit_by_str` で対象コミットの
`root_hash` を引き、`ProllyTree::from_root(root_hash, store)` でその時点の
ツリーを再構築して読み出す。移植時に持っていく場合は
`aruaru-core::version::prolly::ProllyTree::from_root`(既存)+
`VersionController::get_commit_by_str`(新規)+ `aruaru-query`側の2ファイルを
セットで持っていくこと。**スコープの限界**: 単一行(PK一致のWHERE)のみ、
pgwire経由(`open-runo`/`open-web-server`からの外部呼び出し)へは未配線
(SQL層の実装のみ、ネットワーク越しの呼び出しはこのパスの範囲外)。

### 4.3 既存 Postgres/MySQL/CSV からの移行

```rust
use aruaru_migrate::{from_postgres, from_mysql, from_csv};

from_postgres::run_migration(pg_conn_str, target_client, &tables)?;
```

CLI からも: `cargo run -p aruaru-migrate -- --source postgres://... --tables users,orders`。
Snowflake へのエクスポートは Parquet 経路を共有(`docs/DATABASE.md` 参照)。

### 4.4 バックアップ・復元

```rust
use aruaru_backup::{BackupEngine, BackupConfig, BackupDestination, BackupKind, BackupCompression};

let config = BackupConfig {
    destination: BackupDestination::Local { path: "./backups".into() },
    kind: BackupKind::Full,
    compression: BackupCompression::None,
    encrypt: false,
    retention_days: 7,
};
let engine = BackupEngine::new(config, query_engine.clone());
engine.run_full(|_progress| {}).await?;   // フルスナップショット(Parquet + SHA-256)
engine.restore("2026-07-11T00-00Z", &"./data".into(), |_progress| {}).await?;
```

現状はフルダンプ方式(CoW 差分保存は未実装、`CLAUDE.md` 参照)。S3宛先は
2026-07-12実装済み(`BackupDestination::S3`、認証情報は
`AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`環境変数から取得)。SFTP宛先は
未接続。

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

## 8. 移植・拡張時の注意

他プロジェクトへ移植・拡張する際、依存クレートの現状(バージョン・
メンテナンス状況)や仕様の詳細で判断に迷う場合は、学習データからの
推測だけに頼らず、実際に検索(日本語・英語の両方の Google 検索、
および GitHub 上の実装状況調査)して裏付けを取ってから技術選定を
行うことを推奨する。

**パフォーマンス・並行処理**: 移植先でも、非同期(tokio マルチスレッド
ランタイム)を基本としつつ、必要な場面のみ同期処理を用いる方針を
踏襲すること。`#[tokio::main]` のランタイム flavor が
`current_thread` に固定されていないか、async 関数内でブロッキング
I/O・CPU 負荷処理を直接呼んでいないか(`tokio::task::spawn_blocking`
へ退避すべき)、CPU 律速な処理(チェックサム・OLAPクエリ等)は
`rayon`/`DataFusion` の並列実行を活用する価値があるか、を移植時にも
確認するとよい。

**最先端追従の方針(2026-07-23追記)**: 移植時の日英Web検索で
CockroachDB/TiKV等の最先端の実運用システムが既に対応済みと判明した
設計ギャップが見つかった場合、「今のところは大丈夫」という報告に
留めず、確認を求めず自動でそのまま実装に着手すること(ユーザー指示、
正本は`open-raid-z/CLAUDE.md`・`PORTING.md`同日エントリ参照)。
このリポジトリ自身が最初の適用例——Raft単一グループをMulti-Raft
(Range単位の独立Raftグループ)へ追従させた作業を本日のHANDOFFに記録。

**行単位デルタマージによるHTAP列キャッシュ(移植元:
`aruaru-query::olap::OlapCache`、2026-07-23新設、TiFlash Delta Tree
方式)**: 行ストア(OLTP)から列ストア(OLAP)への同期を、テーブル全体
再構築ではなく「変更された行のpkだけ追跡し、ベースの列バッチから
`arrow::compute::filter_record_batch`で除去→変更行だけ再取得した
小さなデルタバッチを`concat_batches`で結合」という設計で行う。
DataFusion(Arrow)を使う他のOLAP/HTAP実装へそのまま移植可能な
パターン——スキーマ変更(CREATE/DROP TABLE)は別途「全体再構築が必要」
集合で扱い、行単位の通常書き込みとは区別すること。
