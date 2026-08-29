# aruaru-DB 🦀

> **2026-08-29 更新(管理面の抜本再設計に移行)**: aruaru-dbは**RPoemと
> SET(対)で使うことで初めて「REST API不要・WunderGraph Cosmo有料版
> (Enterprise)互換」という価値が成立する**設計。「RESTを1本ずつ
> GraphQL mutationへ移す」だけでは *稼働中プロセスの生状態をフィールド
> 単位でライブ書き換えするアンチパターン* の移送にすぎない、という
> 指摘を受け、**正本の設計文書 [`docs/CONTROL_PLANE_REDESIGN.md`]
> (docs/CONTROL_PLANE_REDESIGN.md) を新設**して抜本再設計へ移行した。
> 新・設計哲学(§2、12か条): **すべてを「望ましい状態の宣言」と
> reconciliation で表し、データプレーンに命令的 RPC を置かない**
> (K8s/GitOps・WunderGraph Cosmo・TiDB/TiFlash・SPIFFE の共通解)。
> データプレーン(`aruaru-server`)が公開するHTTPは最終的に `/graphql`・
> `/graphql/sdl`・`/health*`・`/metrics` のみとし、**`/admin/*` を含む
> REST APIを全撤廃**する。運用設定は宣言的 `aruaru.yaml` +
> ホットリロード(実行時ミューテーション廃止)。APIキーは完全自動
> ライフサイクル(自動発行・自動承認・自動破棄・自動削除)。
>
> 進捗(2026-08-29 時点、P0〜P6 のうち): P0 設計確定 / P1 宣言的設定
> 基盤(`aruaru-server::config`、`--config`、ホットリロード)完了 /
> P2 `query.parallel`(4フィールド化)・`follower_read.target_lag_ms`
> (完全ホットリロード)完了 / P3 `/admin/parallel*`・`/v1/keys/self-issue`
> の REST 撤廃完了(GraphQL `explainDistributed`・`parallelJobs`・
> `selfIssueKey` へ)。付録 A に「CockroachDB×Snowflake ハイブリッド
> 変種の実在DB(TiDB/TiFlash 等)」調査、付録 B に「REST撤廃を可能に
> する Cosmo の技術(Federation / Connect / Persisted Operations /
> Schema Registry+CDN)」。詳細・残作業・復活用メッセージは `CLAUDE.md`
> 冒頭「🔄 セッション再開用メモ」と HANDOFF(続き5〜)。
>
> **2026-07-25 更新**: 開発方針ファイル(`CLAUDE.md`)の見出しを
> 「開発方針＆開発環境ルール」から「設計思想＆開発方針＆開発環境ルール」
> へ改名しました。プロジェクトの設計思想(何を大事にしているか)・
> 開発方針(どう進めるか)・開発環境ルール(具体的な運用規約)を明確に
> 区別して記載しています。詳細は`CLAUDE.md`を参照してください。


> **The hybrid distributed database that speaks Git.**  
> CockroachDB の分散強整合 × Snowflake のストレージ/コンピュート分離 × Git-on-SQL バージョン管理 ── すべてを Pure Rust で。

[![Version](https://img.shields.io/badge/version-0.5.0-orange.svg)](https://github.com/aon-co-jp/aruaru-db/releases)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/aon-co-jp/aruaru-db/actions/workflows/ci.yml/badge.svg)](https://github.com/aon-co-jp/aruaru-db/actions)
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

アンインストールは同梱の[uninstall.sh](uninstall.sh)/
[uninstall.ps1](uninstall.ps1)を使用してください(2026-07-30追加)。
実データディレクトリ(`/var/lib/aruaru-db`等)は意図的に削除しません
(別バージョン再インストール時もデータはそのまま利用されます)。

## セキュリティ(2026-07-30重要な修正)

`/admin/*`管理API(cluster/backup/migrate/federation/registry/raft含む)
は、以前は`disaster-email-backup`系エンドポイントのみ認証されており、
大半が無認証でインターネットから到達可能という重大なギャップがあった。
`ARUARU_DB_ADMIN_TOKEN`環境変数+`x-admin-token`ヘッダーによる認証を
`/admin/*`全体へ遡及適用し、定数時間トークン比較(タイミングサイド
チャネル対策)も導入した。**本番運用では`ARUARU_DB_ADMIN_TOKEN`を必ず
設定してください**(未設定の場合、管理APIは503を返し機能しません)。

## Web管理UI(2026-07-30新設、`web/`)

RPoem(`open-runo-poem-compat`、Poem/Tauri非依存)による管理UI。既存の
`aruaru-server`管理API(`/admin/cluster`等)へリバースプロキシする。
ブラウザ↔Web層(`ARUARU_WEB_ADMIN_TOKEN`)・Web層↔aruaru-server
(`ARUARU_UPSTREAM_ADMIN_TOKEN`)の2段階トークン、
`ARUARU_WEB_READ_ONLY=1`によるread-onlyデモモードに対応。

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

**スタンドアロン・メール ディザスタバックアップ(2026-07-25追記)**:
VPS間分散同期・Raftクラスタの複数ノード構成・ZFSスナップショット連携の
いずれも設定しなくても、メールアドレスひとつだけで有効化できる最後の
砦のバックアップ安全網を追加(`crates/aruaru-dist`の`disaster_email_backup`
feature)。`open-raid-z`の`EmailBackupTarget`をそのまま再利用し、管理API
(`POST /admin/disaster-email-backup`、`x-admin-token`認証)から設定する。
ローカルモックSMTPでのテストのみ実施済み(実SMTP・実断線シナリオの検証は
未実施、正直な開示は `CLAUDE.md` の2026-07-25 HANDOFF参照)。

**同日追記**: `RaftWriter::propose_and_wait`のquorum障害(過半数コミット
タイムアウト)発生時に、設定されていれば自動で`DisasterEmailBackup`を
呼ぶ配線を追加。`Option<Arc<DisasterEmailBackup>>`として任意設定
(未設定なら挙動は完全に無変更)、`tokio::spawn`+`spawn_blocking`で
呼び出し元をブロックしないバックグラウンド送信。詳細・検証結果・
残課題は `CLAUDE.md` の同日続編HANDOFF参照。

**同日追記(続)**: 前回開示していた3つの未検証ギャップを解消/前進。
(1) 到達不能ケースだけでなく、TCP接続は確立するがSMTP応答が数秒遅延する
「真の低速SMTP」でも`propose_and_wait`が呼び出し元をブロックしないことを
実測、(2) 管理API(`POST /admin/disaster-email-backup`)が、検証・保管
だけでなく**稼働中のRaftWriterインスタンスへ実際に注入**するよう変更
(`ReplicatedWriter`トレイトへ実行時セッター`set_disaster_email_backup`を
追加)、(3) `RaftNode`を直接叩きRaftWriterを迂回する経路を棚卸しし、
REST `/admin/cluster/propose`の迂回を発見・修正。GraphQL側の
`cluster_propose` resolver(`crates/aruaru-graphql/src/admin_resolvers.rs`)
は依然として`QueryEngine`への直接書き込みでRaftWriterを迂回しており、
今回は未修正(次回候補)。詳細は`CLAUDE.md`の同日3件目HANDOFF参照。

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

## 🕒 Closed Timestamp / Follower Read (2026-08-21 追加)

「CockroachDB の Raft 強整合」と「Snowflake のストレージ/コンピュート分離」を
実際に両立させている共通の要素技術として、*読み取りをリーダー
(leaseholder)以外のレプリカへ逃がす仕組み* が CockroachDB(closed
timestamp)・TiKV/TiDB(safe-ts による Stale Read)・YugabyteDB
(`yb_follower_read_staleness_ms` による bounded staleness)のいずれにも
存在することを調査で確認しました。aruaru-db にはこの概念が**一切
存在しなかった**(コードを grep して裏取り済み)ため、
`crates/aruaru-dist/src/closed_ts.rs` として実装しました。

- Range 単位の closed timestamp を `now - target_lag`(既定3秒)まで前進。
  ただし**進行中の書き込みの最小時刻を跨がない**(跨ぐと保証が破れる)。
- side transport 相当の配布(`publish_to`)で、読み取り専用ノード側の
  コーディネータへ closed timestamp を伝搬(冪等)。
- bounded staleness 交渉: 関与する全 Range の closed timestamp の最小値を
  読み取り時刻に採用。上限超過・未前進・未知 Range は leaseholder へ
  フォールバック(`ReadPlan::RouteToLeaseholder`)。
- `MultiRaftCluster::propose_at` / `commit_and_apply_at` /
  `plan_bounded_staleness_read` として Multi-Raft へ配線済み。

**正直な現状**: 実装・検証は `cargo test -p aruaru-dist`(50 passed /
0 failed)・`cargo test --workspace`(全て ok)までで、(1) 管理REST API への
公開と実プロセスでの E2E 確認、(2) side transport のネットワーク越し配線、
(3) 判定結果を既存の `AS OF COMMIT` 読み取り経路へ橋渡しすること、の3点は
**未実施**です。また時刻は呼び出し側が渡す論理ナノ秒で、HLC・クロック
スキュー上限は扱いません。次回調査・開発の目星(Neon の pageserver/
safekeeper 分離、SingleStore の rowstore/columnstore 同居、Databend/
RisingWave のオブジェクトストレージ直結、CockroachDB/TiDB Serverless の
弾力的コンピュート)は [CLAUDE.md](CLAUDE.md) の HANDOFF 節に整理して
あります。

---

## 🧱 ストレージ/コンピュート分離の要素技術 (2026-08-22 追加)

「Snowflake と CockroachDB の両方の特性を持つハイブリッド DB」の要素技術を
日本語・英語・中国語・韓国語で横断調査し、一次資料で裏取りしたうえで
優先度の高い3件を実装しました(前回追加の closed timestamp / follower read
とは別軸の追加です)。

1. **WAL サービス (safekeeper) と Pageserver の分離 — Neon 方式**
   (`crates/aruaru-dist/src/wal_service.rs`)
   `neondatabase/neon` の `docs/walservice.md`・`docs/safekeeper-protocol.md`・
   `docs/pageserver-storage.md` で確認した設計を実装:
   - WAL は複数 safekeeper へ送り、**過半数が flush した時点で durable**。
     `commitLSN` は全 safekeeper の `flushLSN` を並べた **quorum 番目**。
   - proposer は起動ごとに **term** が増え、古い proposer を fence する
     (= 単一 primary 強制 / split-brain 防止)。
   - Pageserver は safekeeper 群から WAL を取り込み、
     **`get_page_at_lsn` で任意 LSN のページを image layer + delta layer から
     再構成**。未着 LSN の要求は拒否し、`max_replication_lag` で
     バックプレッシャをかける。image layer 生成 (compaction) より前の
     LSN は再構成不能であることを `BelowGcCutoff` として明示。

2. **セグメント (Row Group / block) 単位のゾーンマップ枝刈り**
   (`crates/aruaru-query/src/olap.rs`)
   SingleStore の Universal Storage (segment 単位の columnstore)、
   Databend の block 単位 min/max、DuckDB の Row Group がいずれも
   **セグメント単位統計での部分スキップ**を行っている共通点を確認し、
   従来「テーブル全体で min/max 1組」だった粒度を細分化しました。
   該当行が絶対に無いと証明できたセグメントは DataFusion へ渡さず、
   生き残ったセグメントは `RecordBatch::slice` (ゼロコピー) として
   1パーティションずつ登録します (枝刈りと並列実行が同時に効く)。
   枝刈り結果は `OlapCache::plan_segment_pruning` で観測できます。

3. **オブジェクトストレージ直結のテーブルフォーマット — Databend 方式**
   (`crates/aruaru-backup/src/table_format.rs`)
   中国語一次資料 (「Databend 存储架构总览」「Databend 索引结构说明」) で
   確認した snapshot → segment → block の3層メタデータを実装:
   - `_ss/<32桁16進>_v1.json` (snapshot) / `_sg/<32桁16進>_v1.json`
     (segment、**最大1000 block**) というパス構成。
   - block ごとの min/max (`ColumnStats`) と bloom filter による枝刈り。
     segment 側の集約統計で**丸ごと読み飛ばせる場合は block を一切見ない**。
   - `prev_snapshot_id` の連鎖による時間旅行。
   - **MetaSrv 相当の Snapshot Key の楽観的 CAS が成功して初めてコミット
     成立**(古い親の上への書き込みは `CommitConflict`)。

**正直な現状**: 検証は `cargo test --workspace` / `cargo build --workspace`
までです。(1) `wal_service` は同一プロセス内のオブジェクト分離であり
ネットワーク越しのストリーミング複製・管理 REST API 公開・既存 SQL 経路
(`AS OF COMMIT`) への接続は**未実施**、(2) `table_format` の `ObjectStore`
実装はメモリ上のみで既存の `s3.rs` へ**未接続**、Parquet 実体の書き出しも
行いません、(3) RisingWave の Hummock (epoch/barrier ベースのチェック
ポイント) と SingleStore の hash index / 行レベルロックは**未実装**です。
次回への引き継ぎは [CLAUDE.md](CLAUDE.md) の HANDOFF 節に記載しています。

---

## 📄 ライセンス

Apache License 2.0 — 商用利用・改変・再配布すべて自由。  
© 2026 aruaru-DB Contributors
