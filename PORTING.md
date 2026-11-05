# PORTING.md — aruaru-DB お引越しファイル

> **2026-07-25 更新**: 開発方針ファイル(`CLAUDE.md`)の見出しを
> 「設計思想＆開発方針＆開発環境ルール」へ改名しました
> (設計思想・開発方針・開発環境ルールを明確に区別)。移設先でも
> `CLAUDE.md`の内容を必ず確認してください。


> このファイル 1 枚で、**どのプロジェクトでも aruaru-DB を導入・移設**できます。
> 新しいバックエンド/データ層リポジトリにこのファイルをコピーして、
> 上から順に進めてください。
>
> 対象バージョン: workspace 0.5.0(9 クレート / 76 テスト、2026-07-11 実測。
> 最新のクレート数・テスト数は `CLAUDE.md` の「現状」節参照)
> 最終更新: 2026-07-23

---

## 0. 配布インストーラー(2026-07-23追加)

`install.sh`(Linux、systemdサービス登録)/`install.ps1`(Windows、
要管理者権限)/`.github/workflows/release.yml`(タグpushでLinux
x86_64・Windows x86_64向け`aruaru-server`を自動ビルドしGitHub
Releasesへ添付)を追加。**移植時の注意**: `aruaru-core`が
`../RS-JSON`(`Rust-JSON`リポジトリ)へのpath依存を持つため、CI/別
マシンでビルドする場合は同じ相対位置(このリポジトリのルートの
1つ上)に`RS-JSON`リポジトリをcheckoutしておく必要がある
(`release.yml`内の`git clone`ステップを参照——他のクレート`open_raid_z_core`
のpath依存も同様のパターン、`open_raid_z` feature有効時のみ影響)。

## 0.5. アンインストーラー + 管理API認証の遡及適用 + Web管理UI(2026-07-30追加、重要)

- `uninstall.sh`/`uninstall.ps1`: サービス停止・バイナリ削除のみ行い、
  実データディレクトリ(`/var/lib/aruaru-db`等)は絶対に削除しない設計。
- **セキュリティ上の重大な移植時の注意**: `crates/aruaru-server/src/
  admin.rs`の`admin_routes()`は、以前は`disaster-email-backup`系
  エンドポイントのみ`x-admin-token`認証されており、`cluster`/`backup`/
  `migrate`/`federation`/`registry`/`raft/append`/`raft/vote`を含む
  大半が無認証でインターネットから到達可能という重大なギャップが
  あった。現在は`admin_routes()`が返す`Route`全体を`.around()`
  ミドルウェアで包み、`check_admin_auth`(定数時間トークン比較、
  CWE-208対策)を全エンドポイントへ適用済み。**移植先でも`/admin/*`
  相当のエンドポイントを追加する際は、必ずこの`.around()`ラッパーの
  適用範囲に含まれることを確認すること**(個別ハンドラごとに
  認証を書き忘れるパターンの再発防止)。`ARUARU_DB_ADMIN_TOKEN`
  環境変数が未設定の場合は503を返す(fail-closed設計)。
- `web/`(新規独立クレート): RPoem(`open-runo-poem-compat`)による
  管理UI。既存の`aruaru-server`管理APIへのリバースプロキシとして
  実装、2段階トークン(ブラウザ↔Web層・Web層↔aruaru-server)+
  read-onlyデモモードに対応。他プロジェクトでも「常駐デーモンの
  既存管理APIへの薄いリバースプロキシ」パターンとして移植可能。

---

## 0.6. 自動アップデート機能(2026-08-20、既定off)

`crates/aruaru-server/src/self_update.rs`: GitHub Releases API
(`aon-co-jp/aruaru-db`)を問い合わせ、semver比較で新版を検知すると
プラットフォーム別アセット(`aruaru-db-windows-x86_64.zip`/
`aruaru-db-linux-x86_64.tar.gz`)をダウンロード・展開し、新バイナリを
起動時と同じコマンドライン引数で起動、`GET /healthz`
(このコミットで新設)への到達を`HEALTH_CHECK_SECS`(12秒)以内に確認
できなければ旧バイナリへ自動ロールバックする。`open-english`/
`aruaru-llm`/`rs-sync`/`RPoem`と同じ設計思想。**既定で無効**
(`ARUARU_DB_ENABLE_SELF_UPDATE=1`を明示設定しない限り何もしない)——
常駐DBサーバーである本リポジトリの特性上、意図せぬ自己更新による
不意の再起動を避けるため他リポジトリより慎重な既定off。移植時は
(1) `.github/workflows/release.yml`が同じ命名規則でアセットを
添付する構成になっているか、(2) `/healthz`相当のエンドポイントが
存在するか、を確認すること。実機でのGitHub Release検知→自己更新→
ロールバックの一気通貫E2E検証は未実施(`/healthz`単体の実HTTP応答
確認のみ実施済み、詳細は`CLAUDE.md`2026-08-20 HANDOFF参照)。

## 0.1. aruaru-DB とは何か・エコシステム内での位置づけ

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
# aruaru-core = { git = "https://github.com/aon-co-jp/aruaru-db" }

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

> **⚠️ 2026-08-29 抜本再設計中**: 管理面(`/admin/*` の REST)は**全撤廃**の
> 方針へ転換した。正本は [`docs/CONTROL_PLANE_REDESIGN.md`](docs/CONTROL_PLANE_REDESIGN.md)。
> 最終形の `aruaru-server` の HTTP は `/graphql`・`/graphql/sdl`・
> `/health*`・`/metrics` のみ。運用設定は宣言的 `aruaru.yaml` +
> ホットリロード(`aruaru-server::config`、`--config <path>`)。以下の表の
> `admin/`(Tauri)行と `/admin/*` は移行途上——最新状況は
> `CLAUDE.md` 冒頭「🔄 セッション再開用メモ」参照。

| インターフェース | 用途 |
|------|------|
| `psql -h <host> -p 5432` | PostgreSQL ワイヤ互換(pgwire) |
| `POST /graphql`(`GET /graphql` で GraphiQL) | Versionless GraphQL(query=観測 / mutation=アクション) |
| `--config aruaru.yaml`(ホットリロード) | 宣言的な運用設定(旧 `/admin/*` 設定系の移行先) |
| `SELECT aruaru_branch/commit/diff/merge/log(...)` | Git-on-SQL バージョン管理 |
| `cargo run -p aruaru-migrate` | 他エンジンからの移行 CLI |
| `cargo run -p aruaru-backup` | バックアップ・リストア CLI |
| `admin/`(Tauri) | 管理 GUI(GraphQL クライアント化へ移行中) |

移設先で pgwire/GraphQL のどちらかしか要らない場合は、対応するクレート
(`aruaru-wire` または `aruaru-graphql`)だけを依存に加えれば足りる
(いずれも `aruaru-core`/`aruaru-query` を共通基盤として利用)。

**⚠️ 移植・拡張時の大前提(2026-08-29、ユーザー指示によりCLAUDE.md冒頭に
固定した最重要事項をここにも明記)**: aruaru-dbは**RPoemとSET(対)で
使うことで初めて「REST API不要・WunderGraph Cosmo有料版(Enterprise)
互換」という価値が成立する**設計である。以下のREST→GraphQL段階移行は
その価値(SCIM/SSO相当・APIキー自動管理・VersionlessAPI互換)を強化
する目的で行っており、**REST APIの代替をただ闇雲に作る作業ではない**
——移植先で同種の作業を行う場合も、必ず「これはRPoemとのSET連携の
価値を強化するものか」を自問してから着手すること。WunderGraph Cosmo
**本体**(Router等)は実はApache 2.0のOSSであり、有料(Enterprise)
部分はSSO+SCIM・専有クラウドに限定される点も忘れないこと。

**REST→GraphQL の状況(2026-08-29、抜本再設計へ移行後)**: 当初は
`/admin/*` を 1 本ずつ「GraphQL + 共有 `Arc<Mutex<..>>`」へ移していたが、
それは *稼働中プロセスの生状態をフィールド単位でライブ書き換えする
アンチパターン* の移送にすぎない、との指摘を受けて方針転換。
正本 [`docs/CONTROL_PLANE_REDESIGN.md`](docs/CONTROL_PLANE_REDESIGN.md)
に基づき、**設定系(B1)は宣言的 `aruaru.yaml` + reconcile へ、
アクション系(B2)は GraphQL Mutation、観測系(B3)は GraphQL Query、
ノード間 RPC(B4)はバイナリトランスポート**、と 4 バケツに仕分けて
`/admin/*` を全撤廃していく。移植先で管理系機能を足す場合は、まず
「その項目は望ましい状態(宣言)か、一度きりの副作用(RPC)か」を
§2.2 の判断フローで分類すること。
- **撤廃済み**: `object-table`(`objectTable`/`objectTableCommit`/
  `objectTablePrune`)、`keys`(`keyStatus`/`revokeKeys`)、
  `/admin/parallel*`(設定は `aruaru.yaml: query.parallel`、
  実効値/プラン/ジョブは `parallelConfig`/`explainDistributed`/
  `parallelJobs` query)、`/v1/keys/self-issue`(`selfIssueKey` mutation、
  認証ガード無し)。**2026-08-31(続き10)追加**: `closed-timestamp`
  (`closedTimestamp`/`planFollowerRead` query、`closedTsRegisterRange`/
  `closedTsAdvance` mutation。`/receive`・`/publish` は B4 として残置)、
  `wal-service`(`walService`/`walPage` query、`walAppend`/
  `walCreateImageLayer` mutation)、`sharded-store`(`shardedStoreGet`/
  `shardedStoreStats` query、`shardedStorePut` mutation)。タイムスタンプ・
  LSN は u64 のため GraphQL では String 表現。
- **設定へ移行済み**: `backup.schedule`・`federation.sources`・
  `query.parallel`・`follower_read.target_lag_ms`(`aruaru-server::config`
  の `reconcile` がホットリロード反映)。`wal`・`sharded_store` は
  構築時固定のため静的扱い(`restart_required`)。
- **未着手(P3 残り、次スライス)**: `ephemeral-query`・`multi-raft`
  (split/merge/scatter-query)。いずれも `aruaru-server` 固有の型に依存
  (`ephemeral-query` は `current_exe` で自プロセス再起動、`multi-raft` は
  `MultiRaftCluster<crate::cluster::EngineApplier>` の server-local
  ジェネリック)——`AdminCtx` へ `Arc<dyn EphemeralRunner>` /
  `Arc<dyn MultiRaftHandle>` の trait を注入するリファクタが必要
  (状態注入だけで済んだ上記 3 群とは別スライス。理由は
  `docs/CONTROL_PLANE_REDESIGN.md` §8 P3 に明記)。
- **注意**: `registry`(`crawlRegistry`/`testRegistryConnection`)は
  既に GraphQL 側で実データ接続済み——「未着手」と誤解しないこと。

**付録 A(2026-08-31 大幅拡充)= データプレーン設計の指針**: `docs/
CONTROL_PLANE_REDESIGN.md` 付録 A を「CockroachDB × Snowflake ハイブリッド
変種」の 2026 年最新設計として再構成した(英・日・独で一次論文/GitHub
再調査)。移植先で HTAP・列指向・ストレージ分離を実装する場合は、
TiDB/TiFlash の DeltaTree、CockroachDB の closed timestamp/Pebble、Neon の
safekeeper/pageserver 分離、ClickHouse SharedMergeTree(Keeper = 共有
メタデータの真実源)、Iceberg/Delta/Hudi の table format、Photon/DuckDB の
型認識軽量圧縮(FSST/ALP)を**実装方法まで**参照できる。aruaru-db が
今後取り込むと決めたもの: Raft-Learner 上の行→列非同期変換レプリカ
(本命、`ColumnarApplier` 案)/ HLC / deletion vector。宣言的設定
(`aruaru.yaml: htap` 案)+ execution-config 配信で管理面に載る(§2 原則 12)。

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

**スタンドアロン・メールディザスタバックアップ(移植元: `open-raid-z`の
`offsite_backup::EmailBackupTarget`、2026-07-25新設)**: VPS間分散同期・
クラスタ構成・スナップショット連携のいずれも不要な、メールアドレス
ひとつだけで有効化できる最後の砦のバックアップ。`aruaru-dist`側の
`disaster_email_backup.rs`は、`open_raid_z_core::offsite_backup`への
path依存を再利用し、独自クレートの書き込みコマンド型(このリポジトリでは
`raft::Command`)をJSON化してメール添付するだけの薄いラッパー。他の
プロジェクトへ移植する際も、メール送信ロジック自体(lettre経由のSMTP)は
再実装せず、`open_raid_z_core`へのpath依存を追加する形で再利用すること。
管理APIの認証(`x-admin-token`ヘッダー、環境変数トークン未設定なら503)は
`open-web-server`/`open-easy-web`と共通の規約——新しい認証方式を増やさず
既存パターンをそのまま踏襲する。

**quorum障害への自動配線(同日追記)**: `raft/writer.rs::RaftWriter`に
`Option<Arc<DisasterEmailBackup>>`を持たせ、`wait_for_commit`が
真にquorum未達(タイムアウト)で`Err`を返した経路でのみ
`backup_failed_command`を呼ぶ設計にした(Applier側が拒否した非quorum
エラーでは呼ばない)。送信自体はブロッキングI/Oのため
`tokio::spawn`+`tokio::task::spawn_blocking`で呼び出し元から完全に
切り離す。他プロジェクトへ移植する際、`ReplicatedWriter`/`RaftWriter`
に相当するジェネリックな書き込みラッパーがあれば、同じ
「Option型フィールド+quorum失敗パスでのみfire-and-forget呼び出し」
というパターンをそのまま踏襲できる。

## Closed Timestamp / Follower Read の移植メモ(2026-08-21追記)

`aruaru-dist/src/closed_ts.rs`(新設)は、CockroachDBのclosed timestamp・
TiKVのsafe-ts・YugabyteDBの`yb_follower_read_staleness_ms`に相当する
「リーダー以外のレプリカが自力で読み取ってよい時刻の上限」を管理する
**外部依存ゼロ・Raft実装非依存**のモジュール。時刻は呼び出し側が渡す
論理ナノ秒(`Timestamp = u64`)で、HLCやクロックスキュー上限は扱わない。
そのため他プロジェクトへ移植する際は、`RaftNode`相当の実装を持ち込む
必要はなく、`ClosedTimestampCoordinator`単体を持っていけば
「read_ts <= closed_ts なら読み取り専用ノードで応答してよい」という
安全性ゲートだけを再利用できる。`MultiRaftCluster`との配線
(`propose_at`/`commit_and_apply_at`/`plan_bounded_staleness_read`)は
本リポジトリ固有の薄い接続層であり、移植時は書き込み開始・完了の
2点にフックを入れる形へ読み替えること。

`aruaru-migrate`側については今回新たな発見はない(移行アダプタの
インタフェースは変更していない)。

## WAL サービス/Pageserver・セグメント統計・オブジェクトテーブルの移植メモ(2026-08-22追記)

- **`aruaru-dist/src/wal_service.rs`(Neon 方式、新設)**: `Safekeeper`
  (term fencing + `flush_lsn`)・`WalService`(quorum 番目の `flushLSN` を
  `commitLSN` とする)・`Pageserver`(`get_page_at_lsn` によるページ再構成、
  image/delta layer、バックプレッシャ)・それらを束ねる
  `DisaggregatedStorage`から成る。**外部依存は `parking_lot` と
  `thiserror` のみで、既存の Raft 実装・ストレージ層に一切依存しない**
  ため、他プロジェクトへはファイル1つを持っていくだけで移植できる。
  逆に言えば本リポジトリの SQL 実行経路とも未接続なので、移植先で
  「書き込み時に `write()` を呼び、読み取り時に `get_page_at_lsn` を
  経由させる」配線は移植先側で書く必要がある。
- **`aruaru-query/src/olap.rs`(セグメント単位ゾーンマップ、改修)**:
  `OlapCache`に`segment_rows`(既定1024)を追加し、`SegmentStats`
  (offset/len/min-max)で部分スキップする形へ変更。Arrow の
  `RecordBatch::slice`と DataFusion の`MemTable`(パーティション配列)に
  依存するため、移植先が Arrow/DataFusion を使っていない場合は
  「セグメント境界と min/max を持ち、証明できたセグメントをスキャン
  対象から外す」という考え方だけを移すこと。
- **`aruaru-backup/src/table_format.rs`(Databend 方式、新設)**:
  `ObjectStore`トレイト(`put`/`get`/`list`の3メソッドのみ)を差し替え
  可能な形にしてあるため、移植先の任意のオブジェクトストレージ
  クライアントを実装して差し込める(同梱はメモリ実装のみ)。
  依存は`serde`/`serde_json`/`sha2`/`hex`/`chrono`/`thiserror`。
  コミットの原子性は`MetaService`の CAS 1点に集約してあるので、
  移植先が etcd/FoundationDB 等の CAS を持つ場合は`MetaService`だけを
  差し替えれば分散環境でもそのまま成立する設計。

## API 方針: REST 全廃・gRPC / Connect へ (2026-11-04)

### 日本語
- **REST は全廃する。新しい API は REST で作らない。** ブラウザや `curl` から呼ぶ API は **Connect の単純呼び出し**(`POST /<パッケージ>.<Service>/<Method>`、`content-type: application/json`、本文は JSON)、サービス間は **gRPC / gRPC-web**(RPoem の管理API `open_runo.admin.v1.Admin` と同じ流儀)。ストリーミングは、必要になってから。
- **既存の REST は、触るときに順に移す。** 移したら旧 REST の経路は消す。移行が終わるまで残るが、**新規には追加しない**。
- **JSON は RS-JSON に統一**する(読み込みは `from_str_secure` / `parse_secure` でサイズ・深さ・重複キーの上限つき、書き出しは `rust_json::fast`)。ブラウザの WASM に `prost` は入れない(サイズが増えるため)。
- **例外(自分たちの API ではなく、相手の規格が決めているもの)**: ACME、OAuth/OIDC のコールバック、第三者(GitHub など)からの Webhook、ヘルスチェック、静的ファイル配信、OTLP など。これらは規格どおりの HTTP を受ける。
- 参考実装: `RPoem`(`open_runo.admin.v1.Admin`)、`open-easy-web`(`server/src/connect.rs`、`proto/open_easy_web/admin/v1/admin.proto`)。
- ※ このリポジトリのコードが既に方針どおりかどうかは、各リポジトリの現状による。この節は方針の記録であり、コードの変更を意味しない。

### English
- **REST is being abolished. New APIs are not built as REST.** APIs called from browsers or `curl` use **Connect unary calls** (`POST /<package>.<Service>/<Method>`, `content-type: application/json`, JSON body); service-to-service calls use **gRPC / gRPC-web** (the same style as RPoem's admin API `open_runo.admin.v1.Admin`). Streaming only when it is actually needed.
- **Existing REST endpoints are migrated one by one, when they are touched.** The old REST route is removed once migrated. Until migration is finished they remain, but **no new ones are added**.
- **JSON is unified on RS-JSON**: reading with `from_str_secure` / `parse_secure` (size, depth and duplicate-key limits), writing with `rust_json::fast`. `prost` is not put into browser WASM (it would grow the binary).
- **Exceptions (shapes dictated by someone else's standard, not our own API)**: ACME, OAuth/OIDC callbacks, webhooks from third parties (e.g. GitHub), health checks, static file serving, OTLP, and so on. These accept plain HTTP as the standard requires.
- Reference implementations: `RPoem` (`open_runo.admin.v1.Admin`) and `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Note: whether the code in a given repository already follows this policy depends on that repository. This section records the policy; it does not mean the code has been changed.

### Deutsch (KI-Übersetzung, redaktionell überarbeitet; noch nicht von Muttersprachlern geprüft)
- **REST wird vollständig abgeschafft; neue APIs entstehen nicht mehr als REST.** APIs, die aus dem Browser oder per `curl` aufgerufen werden, verwenden **unäre Connect-Aufrufe** (`POST /<Paket>.<Service>/<Methode>`, `content-type: application/json`, JSON-Body). Zwischen Diensten kommen **gRPC bzw. gRPC-web** zum Einsatz, im selben Stil wie die Admin-API von RPoem (`open_runo.admin.v1.Admin`). Streaming wird erst ergänzt, wenn es tatsächlich gebraucht wird.
- **Bestehende REST-Endpunkte werden schrittweise umgestellt, und zwar immer dann, wenn man ohnehin an ihnen arbeitet.** Sobald ein Endpunkt umgestellt ist, wird die alte REST-Route entfernt. Bis zum Abschluss der Migration bleiben die übrigen bestehen, es werden aber **keine neuen mehr hinzugefügt**.
- **JSON wird einheitlich über RS-JSON verarbeitet**: Gelesen wird mit `from_str_secure` bzw. `parse_secure` (mit Obergrenzen für Größe, Verschachtelungstiefe und doppelte Schlüssel), geschrieben mit `rust_json::fast`. `prost` wird nicht in das Browser-WASM aufgenommen, da es die Datei vergrößern würde.
- **Ausnahmen (Schnittstellen, deren Form ein fremder Standard vorgibt und die nicht zu unserer eigenen API gehören)**: ACME, OAuth/OIDC-Callbacks, Webhooks von Drittanbietern (z. B. GitHub), Health-Checks, die Auslieferung statischer Dateien, OTLP und Ähnliches. Diese nehmen, wie vom jeweiligen Standard verlangt, gewöhnliches HTTP entgegen.
- Referenzimplementierungen: `RPoem` (`open_runo.admin.v1.Admin`) und `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Hinweis: Ob der Code eines einzelnen Repositorys diese Richtlinie bereits erfüllt, hängt vom jeweiligen Repository ab. Dieser Abschnitt dokumentiert lediglich die Richtlinie; er besagt nicht, dass der Code schon angepasst wurde.

### Русский (перевод выполнен с помощью ИИ и отредактирован; носителями языка пока не проверен)
- **От REST мы полностью отказываемся; новые API больше не создаются в стиле REST.** API, к которым обращаются из браузера или через `curl`, используют **унарные вызовы Connect** (`POST /<пакет>.<Service>/<Method>`, `content-type: application/json`, тело запроса — JSON). Для взаимодействия между сервисами применяются **gRPC и gRPC-web** — в том же стиле, что и административный API RPoem (`open_runo.admin.v1.Admin`). Потоковая передача данных добавляется лишь тогда, когда в ней действительно возникает необходимость.
- **Существующие REST-эндпоинты переводятся поэтапно — по мере того, как с ними приходится работать.** После перевода старый REST-маршрут удаляется. Пока миграция не завершена, оставшиеся эндпоинты сохраняются, но **новые не добавляются**.
- **Работа с JSON унифицирована на основе RS-JSON**: чтение — через `from_str_secure` или `parse_secure` (с ограничениями на размер, глубину вложенности и повторяющиеся ключи), запись — через `rust_json::fast`. `prost` в браузерный WASM не включается, поскольку это увеличило бы размер файла.
- **Исключения (интерфейсы, форму которых определяет чужой стандарт, а не наш собственный API)**: ACME, обратные вызовы OAuth/OIDC, вебхуки сторонних сервисов (например, GitHub), проверки работоспособности, раздача статических файлов, OTLP и тому подобное. Такие интерфейсы принимают обычный HTTP, как того требует соответствующий стандарт.
- Эталонные реализации: `RPoem` (`open_runo.admin.v1.Admin`) и `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Примечание: соответствует ли код конкретного репозитория этой политике, зависит от самого репозитория. Данный раздел лишь фиксирует политику и не означает, что код уже изменён.

## API 方針の補足: 用語の整理と使い分け (2026-11-04)

### 日本語
**1つに統一するものではなく、別の層の話です。** 「REST の代わり」になるのは Connect・gRPC・GraphQL だけで、WASM は画面を作る技術です。

| 用語 | 何か | REST の代わりか |
|---|---|---|
| REST | URL と HTTP メソッドで操作を表す API の作り方 | 全廃する側 |
| **Connect** | `POST /サービス名/メソッド名` に JSON を送る形式。`curl` でもブラウザの `fetch` でも呼べる | はい(**採用**) |
| **gRPC** | 同じ「サービス/メソッド」の考え方を、バイナリ(protobuf)と HTTP/2 で行う形式。速く、ストリーミングも得意 | はい(**サービス同士に採用**) |
| gRPC-web | gRPC のブラウザ向けの変形版 | はい(必要なら) |
| GraphQL | 欲しい項目を問い合わせる形式 | 一部だけ(**問い合わせ(読み取り)専用**) |
| WASM | Rust をブラウザの中で動かす仕組み | いいえ(画面を作る技術) |

- **ブラウザの画面や `curl` から呼ぶ API は Connect**(POST、JSON を RS-JSON で読み書き)。ブラウザの WASM は素の gRPC を話せず、Connect なら `fetch` のままで、`prost` を足さずに済む(画面のサイズも増えない)。
- **サービス同士の通信(サーバー間、重い処理、ストリーミング)は gRPC。** 多くは Connect で足りるので、ストリーミングが必要になった所だけ gRPC に上げる。
- **GraphQL は、まとめて問い合わせたいときの読み取り専用。** 書き込み(操作)には使わない。
- **WASM は画面を作る技術として、そのまま使う。** 画面から呼ぶ API が Connect になっただけ。
- 外部の規格が形を決めるもの(ACME、OAuth のコールバック、第三者の Webhook、ヘルスチェック、静的ファイル、OTLP)は、規格どおりの HTTP のまま。
- **迷ったら Connect。**

### English
**These are not alternatives to unify; they sit at different layers.** Only Connect, gRPC and GraphQL can replace REST; WASM is a way to build the UI.

| Term | What it is | Replaces REST? |
|---|---|---|
| REST | API style that expresses operations through URLs and HTTP methods | The thing being abolished |
| **Connect** | `POST /Service/Method` with a JSON body; callable from `curl` and from a browser `fetch` | Yes (**adopted**) |
| **gRPC** | The same service/method idea over binary (protobuf) and HTTP/2; fast, good at streaming | Yes (**for service-to-service**) |
| gRPC-web | A browser-friendly variant of gRPC | Yes (if needed) |
| GraphQL | A query language: ask for exactly the fields you want | Partly (**read-only queries**) |
| WASM | Runs Rust inside the browser | No (a UI technology) |

- **APIs called from browser screens or `curl` use Connect** (POST, JSON read and written with RS-JSON). Browser WASM cannot speak plain gRPC; with Connect it stays a `fetch`, and `prost` is not needed (the binary does not grow).
- **Service-to-service calls (server to server, heavy work, streaming) use gRPC.** Connect is enough in most places; move a call to gRPC only where streaming is actually needed.
- **GraphQL is read-only**, for queries that gather several pieces of data. It is not used for writes (operations).
- **WASM stays as the way to build the UI.** The API it calls simply became Connect.
- Interfaces whose shape an outside standard dictates (ACME, OAuth callbacks, third-party webhooks, health checks, static files, OTLP) stay plain HTTP.
- **When in doubt, use Connect.**

### Deutsch (KI-Übersetzung, sprachlich überarbeitet; noch nicht von Muttersprachlern geprüft)
**Es geht nicht darum, eine einzige Technik auszuwählen – die Begriffe liegen auf unterschiedlichen Ebenen.** Als Ersatz für REST kommen nur Connect, gRPC und GraphQL in Frage; WASM dagegen ist eine Technik, mit der die Benutzeroberfläche gebaut wird.

| Begriff | Was ist das? | Ersatz für REST? |
|---|---|---|
| REST | Ein API-Stil, bei dem Operationen über URLs und HTTP-Methoden ausgedrückt werden | wird abgeschafft |
| **Connect** | `POST /Dienst/Methode` mit JSON im Body; aufrufbar mit `curl` ebenso wie mit `fetch` im Browser | ja (**gewählt**) |
| **gRPC** | Dasselbe Prinzip aus Dienst und Methode, aber über binäres protobuf und HTTP/2; schnell und gut für Streaming geeignet | ja (**zwischen Diensten**) |
| gRPC-web | Eine für den Browser angepasste Variante von gRPC | ja (bei Bedarf) |
| GraphQL | Abfragesprache, mit der man genau die benötigten Felder anfordert | teilweise (**nur für lesende Abfragen**) |
| WASM | Führt Rust-Code direkt im Browser aus | nein (Technik für die Oberfläche) |

- **APIs, die von Browseroberflächen oder per `curl` aufgerufen werden, verwenden Connect** (POST; JSON wird mit RS-JSON gelesen und geschrieben). WASM im Browser kann kein reines gRPC sprechen. Mit Connect bleibt es bei einem gewöhnlichen `fetch`, und `prost` ist nicht nötig – die WASM-Datei wird dadurch nicht größer.
- **Für die Kommunikation zwischen Diensten (Server zu Server, rechenintensive Aufgaben, Streaming) wird gRPC eingesetzt.** In den meisten Fällen reicht Connect aus; auf gRPC wird nur dort umgestellt, wo Streaming tatsächlich gebraucht wird.
- **GraphQL dient ausschließlich lesenden Abfragen**, wenn mehrere Informationen gebündelt abgefragt werden sollen. Für Schreibzugriffe (Operationen) wird es nicht verwendet.
- **WASM bleibt die Technik für die Oberfläche.** Geändert hat sich lediglich, dass die aufgerufene API nun auf Connect beruht.
- Schnittstellen, deren Form durch einen externen Standard festgelegt ist (ACME, OAuth-Callbacks, Webhooks von Drittanbietern, Health-Checks, statische Dateien, OTLP), bleiben gewöhnliches HTTP.
- **Im Zweifel: Connect.**

### Русский (перевод выполнен с помощью ИИ и литературно отредактирован; носителями языка пока не проверен)
**Это не варианты, из которых нужно выбрать один, а технологии разных уровней.** Заменить REST могут только Connect, gRPC и GraphQL; WASM же — это способ создания пользовательского интерфейса.

| Термин | Что это | Заменяет REST? |
|---|---|---|
| REST | Стиль построения API, при котором операции выражаются через URL и HTTP-методы | отменяется |
| **Connect** | `POST /Сервис/Метод` с телом в формате JSON; вызывается как из `curl`, так и из браузерного `fetch` | да (**выбран**) |
| **gRPC** | Тот же принцип «сервис — метод», но поверх двоичного формата protobuf и HTTP/2; работает быстро и хорошо подходит для потоковой передачи | да (**для связи между сервисами**) |
| gRPC-web | Адаптированный для браузера вариант gRPC | да (при необходимости) |
| GraphQL | Язык запросов: клиент запрашивает ровно те поля, которые ему нужны | частично (**только запросы на чтение**) |
| WASM | Позволяет выполнять код на Rust прямо в браузере | нет (технология для интерфейса) |

- **API, которые вызываются из браузерных интерфейсов или через `curl`, строятся на Connect** (POST; JSON читается и записывается через RS-JSON). Браузерный WASM не умеет работать с «чистым» gRPC. С Connect достаточно обычного `fetch`, а `prost` не требуется — размер WASM-файла не увеличивается.
- **Для обмена между сервисами (сервер — сервер, ресурсоёмкие задачи, потоковая передача) применяется gRPC.** В большинстве случаев достаточно Connect; на gRPC переходят только там, где потоковая передача действительно необходима.
- **GraphQL используется только для чтения**, когда нужно получить сразу несколько связанных данных. Для записи (операций) он не применяется.
- **WASM по-прежнему остаётся технологией для интерфейса.** Изменилось лишь то, что вызываемый им API теперь построен на Connect.
- Интерфейсы, форма которых задана внешним стандартом (ACME, обратные вызовы OAuth, вебхуки сторонних сервисов, проверки работоспособности, статические файлы, OTLP), остаются обычным HTTP.
- **Если есть сомнения — выбирайте Connect.**

## REST→Connect 移行(2026-11-05)
管理操作は `POST /open_runo.aruaru.admin.v1.Admin/<Method>`(21メソッド)、ノード間配布・メール退避は `.../AdminNode/<Method>`、デモwebは `/open_runo.aruaru.web.v1.Demo/{Status,Rebalance}`、列レプリカ観測は `/open_runo.aruaru.columnar.v1.Columnar/{Status,Prune}` の Connect 単純呼び出し(JSON本文、`x-admin-token`、JSONはRS-JSON `parse_secure`)。旧 `/admin/*` のREST(ノード間 `raft`・`closed-timestamp/receive` を含む)は削除済み。例外は `/healthz`・`/graphql/sdl`・PostgreSQLワイヤー・`/graphql`(問い合わせ)。本番(VPS)反映の手順: glibc 2.39 以下で作るため **Ubuntu 24.04(WSL `Ubuntu-24.04`)でリリースビルド** → `objdump -T` で GLIBC_2.39 以下を確認 → scp → バックアップ → 入れ替え → 再起動 → curl(401/200/400/旧REST 404)。詳細・未了は `CLAUDE.md` の「REST→Connect移行状況」。

