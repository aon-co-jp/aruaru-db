# aruaru-DB 🦀

> **2026-08-29〜31 更新(管理面の抜本再設計 = 2026 年最新設計として進行中)**:
> `/admin/*` の REST を GraphQL へ 1 本ずつ移す手法は *アンチパターンの移送* に
> すぎない、との指摘を受け、正本の設計文書
> **[`docs/CONTROL_PLANE_REDESIGN.md`](docs/CONTROL_PLANE_REDESIGN.md)** を新設して
> 抜本再設計へ移行。新・設計哲学: **すべてを「望ましい状態の宣言」+
> reconciliation で表し、データプレーンに命令的 RPC を置かない**
> (K8s/GitOps・WunderGraph Cosmo・TiDB/TiFlash・SPIFFE の共通解)。
> `aruaru-server` の HTTP は最終的に `/graphql`・`/graphql/sdl`・`/health*`・
> `/metrics` のみとし、**`/admin/*` を含む REST を全撤廃**。運用設定は
> 宣言的 `aruaru.yaml` + ホットリロード。
> **2026-08-31**: 付録 A を「CockroachDB × Snowflake ハイブリッド変種」の
> 2026 年最新設計として大幅拡充(英・日・独で一次論文/GitHub を再調査、
> TiDB/TiFlash の DeltaTree・CockroachDB の closed timestamp/Pebble・
> Snowflake の不変マイクロパーティション・Neon vs Aurora の WAL 分離・
> ClickHouse SharedMergeTree・Iceberg/Delta/Hudi・Photon/DuckDB の
> 型認識軽量圧縮を**実装方法まで**整理し、Raft-Learner 行→列変換レプリカ・
> HLC・deletion vector の取り込みを決定)。進捗: **P0〜P3 完了(REST完全撤廃
> は`closed-timestamp`・`wal-service`・`sharded-store`・`ephemeral-query`・
> `multi-raft`の全5機能で完了、実プロセスHTTP E2E込み)**。続けて要求③
> 実装トラックへ着手: **HLC**(`aruaru-dist::hlc`)、**ColumnarApplier**
> (A.6-2本命、Raft-Learner上の行→列非同期変換レプリカ。`--columnar-learner`
> で実際に2プロセス間の実HTTPまで検証済み)、**deletion vector**
> (A.6-4段階1、`prune_range`/`prune_equality`への配線込み)を実装。
> 詳細・進捗・復活用メッセージは[`CLAUDE.md`](CLAUDE.md)冒頭
> 「🔄 セッション再開用メモ」と同日HANDOFF(続き13〜18)。
> **2026-09-02(続き20〜22)**: **A.6-4 段階2 = base+delta の Merge-on-Read**
> (`ColumnarApplier` を都度フル再構築から delta 蓄積+閾値 compaction へ格上げ、
> DELETE/UPDATE で deletion vector を書き込み)、**HLC 再設計**
> (`as_nanos()` の `pt<<16` u64 オーバーフローを案B〈物理を 65µs 粒度へ
> 切り捨て下位 16bit に論理を収める〉で修正、`closed_ts` 系へ配線)を実装。
> 続いて**次フェーズ一括(ビルドまで)**: `aruaru.yaml: htap` セクション、
> `Query.htapReplicas` 相当の枝刈り込み観測 API
> (`ColumnarApplier::prune_range_preview`/`prune_equality_preview` +
> `GET /columnar/:table/prune`)、**A.6-3**(`Applier::apply_at` で
> Raft index + MVCC〈commit 通し番号〉を記録、`read_at_index` で staleness
> 検証、未達なら 409)、**HLC `max_offset`**(`try_update`/
> `try_observe_ordinal`、`follower_read.max_offset_ms`)を実装。
> HLC 案A 全面移行は `docs/HLC_TIMESTAMP_REDESIGN.md` P-HLC-3 として将来。
> **2026-09-02(続き23)**: `Query.htapReplicas` を GraphQL 単一サーフェスへ
> **正式公開**。`aruaru.yaml: htap.columnar_replicas: true` で本番
> `aruaru-server` が本番 `QueryEngine` を共有した**同居 `ColumnarApplier`**
> (`QueryEngine::set_columnar_observer` の通知で追従)を立て、`htapReplicas`
> が TiFlash `INFORMATION_SCHEMA.TIFLASH_REPLICA`(PROGRESS/AVAILABLE)相当の
> 同期状態 + 枝刈り込みプレビューを返す。設計は TiFlash / CockroachDB の
> レプリカ観測手法の WebSearch 調査を反映。実 HTTP `/graphql` E2E 済み
> (`execSql` で書き込み→自動追従、prune/DELETE/未知テーブル/`max_offset`
> リモート値拒否まで確認)。詳細は [`CLAUDE.md`](CLAUDE.md) HANDOFF(続き20〜23)。

> 📌 保留タスク(2026-08-06): 東芝SBM・DeepSeek技術の組み込み構想あり。詳細は[CLAUDE.md](CLAUDE.md)参照。

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

> **2026-07-27時点の正直な開示**: `admin/`は以前、`tauri.conf.json`・
> `build.rs`・アイコン・Vite/tsconfigのブートストラップ一式が欠落しており
> `npm run build`/`cargo build`のどちらも一度も成功していなかった
> (ページ実装自体は先行していたが、実行できる状態ではなかった)。
> 2026-07-27にこの最小限のスキャフォールドを補い、両方のビルドが実際に
> 成功する状態にした。ディザスタ用メール退避の設定フォームもこの時点で
> 追加済み。実Tauriネイティブウィンドウでの起動確認・実SMTP接続は
> 引き続き未検証(詳細は`CLAUDE.md`のHANDOFF参照)。

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
