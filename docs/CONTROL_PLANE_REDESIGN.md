# 管理面の再設計 — 「REST API 不要」を抜本的に実現する

> ステータス: **設計中（2026-08-29 起票）** / 対象: `aruaru-db` + `RPoem`（SET）
> 決定者: masahiro ishizuka（AON CEO）／ 起案: セッション横断作業
> 関連: [`CLAUDE.md` 冒頭「🎯 最重要・最優先」](../CLAUDE.md) ・ [`PORTING.md`](../PORTING.md)

---

## 0. なぜこの文書があるか（背景）

`aruaru-db` の価値は **RPoem と SET（対）で「REST API 不要・WunderGraph Cosmo
有料版互換」を成立させること**。その過程で `/admin/*` の REST エンドポイントを
1 本ずつ「GraphQL query/mutation ＋ 共有 `Arc<Mutex<..>>`」へ移してきた
（`object-table`・`keys` は撤廃完了）。

しかしこの手法は **アンチパターンをそのまま移送しているだけ** である:

- `setParallelConfig` / `setBackupSchedule` / `registerFederation` /
  `clusterNode` … は、**稼働中プロセスの内部状態をフィールド単位で
  ライブ書き換えする RPC**。プロトコルを REST から GraphQL に変えても、
  「運用設定を実行時ミューテーションで管理する」構造は変わらない。
- 「REST という文字列が消えた」だけで、**宣言的でも冪等でも監査可能でも
  再現可能でもない**。

ユーザー指示（2026-08-29）:
> 「削るだけで、抜本的な解決になっていないなら、Google 検索・GitHub 調査で
> 全てを一から再設計して開発しなおして」「新規設計の、設計思想や設計哲学
> からいちからやり直して」

本文書はその再設計の**正本**。以後、管理面に手を入れる者は必ずここを読む。

---

## 1. 一次資料（WunderGraph Cosmo は実際どうしているか）

- [Cosmo Router configuration](https://cosmo-docs.wundergraph.com/router/configuration)
- [Cosmo overview / architecture](https://cosmo-docs.wundergraph.com/overview)
- [Cosmo Enterprise](https://cosmo-docs.wundergraph.com/enterprise)（有料は SSO+SCIM+専有クラウドのみ、本体は Apache 2.0）

判明した事実:

| 項目 | Cosmo の実際 |
|---|---|
| **データプレーン（Router）が公開する HTTP** | `/graphql`、`/health`・`/health/ready`・`/health/live`、`/metrics`（別リスナ）、`/`（Playground）。**管理用 REST は存在しない。** |
| **静的設定** | YAML 1 枚（listen、TLS、telemetry、feature フラグ、セキュリティ）。`${VAR}` 展開対応。 |
| **動的設定（execution config = 合成スーパーグラフ）** | CDN / コントロールプレーンから**ポーリング取得**（既定 10s）。またはファイル指定。 |
| **ホットリロード** | `SIGHUP` シグナル or `watch_config`（ファイルの定期ポーリング）。プロセス再起動不要。 |
| **管理アクション（schema publish / checks / user 管理）** | **別コンポーネント = Control Plane（Platform API）**。`wgc` CLI と Studio がそこを叩く。**Router 自身には一切載せない。** |
| **データプレーン / コントロールプレーンの分離** | 明確。Router は可用性最優先で Control Plane から独立動作。 |

**結論**: 「運用設定 = 宣言的ドキュメント（静的 YAML ＋ 動的 config を
ホットリロード）」「管理アクション = 別プレーン」「データプレーンは
GraphQL ＋ health ＋ metrics だけ」。

---

## 2. 設計哲学（この再設計が従う原則）

1. **データプレーンは GraphQL に徹する。**
   `aruaru-server` が公開する HTTP は次だけ:
   `/graphql`・`/graphql/sdl`・`/health*`・`/metrics`・`/v1/keys/self-issue`・
   内部 `/raft/*`。**`/admin/*` は 1 本も残さない。**

2. **運用設定は宣言的ドキュメント。実行時ミューテーションではない。**
   `aruaru.yaml`（静的）＋ RPoem コントロールプレーンが配信する
   `execution-config`（動的）。`setX` という mutation は作らない。
   設定変更は「ドキュメントを書き換える → リロードが走る → 反映」。
   冪等・監査可能（Git 管理）・再現可能。

3. **ホットリロードを最初から作り込む。**
   `notify`（ファイル監視）＋ `SIGHUP`。設定の差分だけを稼働中の
   `AppState` へ適用する `reconcile()` を用意する。既存動作を壊さない。

4. **「一度きりのアクション」だけが GraphQL Mutation。**
   `createBackup`・`runMigration`・`rebalanceCluster`・`revokeKeys`・
   `objectTableCommit` … は冪等でない副作用操作＝正当な RPC。
   これらは subgraph の `Mutation` に置く（VersionlessAPI の一部）。

5. **観測は GraphQL Query ＋ Prometheus。**
   `clusterStatus`・`keyStatus`・`parallelJobs`・`objectTable` 履歴 …
   はグラフに載せる。数値カウンタは `/metrics` にも出す。

6. **ノード間 RPC は管理面ではない。**
   `/raft/append`・`/raft/vote`・`/closed-timestamp/receive|publish` は
   クラスタ内部トランスポート。GraphQL 化しない。本再設計の対象外
   （ただし将来はバイナリトランスポートへ寄せる — 別トピック）。

7. **コントロールプレーンは RPoem 側。**
   `RPoem/crates/open-runo-schema-registry`・`open-runo-feature-flags`・
   `open-runo-federation` が「Cosmo の Control Plane ＋ CDN」に相当する
   役割を負う。`aruaru-server` はそこから execution-config を
   **取得する側**（＝ Cosmo Router と同じ立場）。

8. **SET の価値に寄与しない移送はしない。**
   着手前に必ず自問: 「これは SCIM/SSO 相当・APIキー自動管理・
   VersionlessAPI 互換のいずれかを強化するか」。

---

## 3. 目標 HTTP 面（再設計後）

### `aruaru-server`（データプレーン）
```
/graphql              GraphQL（query = 観測 / mutation = アクション / subscription = 将来）
/graphql/sdl          Federation SDL（wgc / RPoem federation が取得）
/health               liveness（200）
/health/ready         readiness（200 / 503）
/health/live          liveness probe
/metrics              Prometheus（別リスナ 127.0.0.1:9090 想定）
/v1/keys/self-issue   認証不要の自己発行（GraphQL 等価なし・維持）
/raft/append /raft/vote   ノード間 RPC（内部）
```
**`/admin/*` は存在しない。**

### RPoem（コントロールプレーン）
```
execution-config の配信（ポーリング or push）      ← open-runo-schema-registry
feature フラグの配信                               ← open-runo-feature-flags
subgraph 合成（Federation composition）            ← open-runo-federation
SCIM 2.0 / SSO(OIDC)（Cosmo Enterprise 相当・OSS） ← open-runo-scim / open-runo-security
APIキー・ライフサイクル（KeyGuardian）             ← open-runo-router::keyring（aruaru-db と設計共有）
```

---

## 4. `/admin/*` 全エンドポイントの 4 バケツ仕分け

分類ルール:
- **B1 宣言的設定** … 「望ましい状態」を表す。何度書いても同じ結果。→ `aruaru.yaml` / execution-config へ。
- **B2 一度きりのアクション** … 冪等でない副作用。→ GraphQL `Mutation`。
- **B3 観測リード** … 状態の読み取り。→ GraphQL `Query`（＋ `/metrics`）。
- **B4 ノード間 RPC** … クラスタ内部通信。→ 対象外（内部トランスポート維持）。

| 旧エンドポイント | バケツ | 移行先 |
|---|---|---|
| `GET/POST /admin/backup/schedule` | **B1** | `aruaru.yaml: backup.schedule` |
| `GET/POST /admin/parallel` | **B1** | `aruaru.yaml: query.parallel`（4 フィールド: enabled/max_workers/chunk_size/strategy） |
| `GET/POST /admin/federation`, `/federation/drop` | **B1** | `aruaru.yaml: federation.sources[]`（登録済み外部ソース一覧） |
| `POST /admin/cluster/node`（配置意図） | **B1** | `execution-config: cluster.nodes[]`（望ましいノード構成） |
| closed-timestamp の `target_lag` | **B1（ホットリロード）** | `aruaru.yaml: follower_read.target_lag_ms`。全 tracker が同一 `Arc<AtomicU64>` を共有し `set_target_lag_nanos` で即反映（P2 実装済み） |
| wal-service の safekeeper quorum | **B1（静的）** | `aruaru.yaml: wal.safekeepers` / `wal.quorum`。台数は構築時固定 → reconcile は `restart_required` を報告するのみ（進行中状態を失わないため意図的） |
| disaster-email-backup の宛先設定 | **B1** | `aruaru.yaml: disaster_backup.email`（feature-gated、P3 で reconcile 接続） |
| sharded-store の shard 数 | **B1（静的）** | `aruaru.yaml: sharded_store.shards`（0 = コア数）。shard 数は構築時固定 → `restart_required` |
| `POST /admin/backup`（作成） | **B2** | `Mutation.createBackup`（実装済み） |
| `POST /admin/backup/restore` | **B2** | `Mutation.restoreBackup`（実装済み） |
| `POST /admin/migrate/run`, `/migrate/instance` | **B2** | `Mutation.runMigration`（実装済み） |
| `POST /admin/cluster/rebalance` | **B2** | `Mutation.rebalanceCluster`（実装済み） |
| `POST /admin/cluster/propose` | **B2** | `Mutation.clusterPropose`（実装済み・RaftWriter 経由） |
| `POST /admin/cluster/node`（実行） | **B2** | `Mutation.clusterNodeOp`（実装済み） |
| `POST /admin/ephemeral-query` | **B2** | `Mutation.ephemeralQuery`（新規） |
| `POST /admin/multi-raft/split`, `/merge` | **B2** | `Mutation.multiRaftSplit` / `multiRaftMerge`（新規） |
| `POST /admin/sharded-store`（put） | **B2** | `Mutation.shardedStorePut`（新規） |
| `POST /admin/closed-timestamp/range`, `/advance` | **B2** | `Mutation.closedTsRegisterRange` / `closedTsAdvance`（新規） |
| `POST /admin/wal-service/append`, `/page`, `/image-layer` | **B2** | `Mutation.wal*`（新規） |
| `POST /admin/registry/crawl` | **B2** | `Mutation.crawlRegistry`（実装済み） |
| `GET /admin/backup`（一覧） | **B3** | `Query.backups`（実装済み） |
| `GET /admin/parallel/jobs` | **B3** | `Query.parallelJobs`（実装済み） |
| `POST /admin/parallel/explain` | **B3** | `Query.explainDistributed`（実装済み・実体は読み取り） |
| `GET /admin/federation`, `/federation/test`, `/federation/query` | **B3** | `Query.federatedSources` / `testSourceConnection` / `federatedQuery`（実装済み） |
| `GET /admin/cluster` | **B3** | `Query.clusterStatus`（実装済み） |
| `GET /admin/multi-raft/scatter-query` | **B3** | `Query.multiRaftScatter`（新規） |
| `GET /admin/sharded-store/:key`, `/sharded-store-stats` | **B3** | `Query.shardedStoreGet` / `shardedStoreStats`（新規） |
| `GET /admin/closed-timestamp`, `POST /admin/closed-timestamp/plan` | **B3** | `Query.closedTimestamp` / `planFollowerRead`（新規） |
| `GET /admin/wal-service` | **B3** | `Query.walService`（新規） |
| `GET /admin/registry`, `/registry/summary`, `/registry/test` | **B3** | `Query.registry` / `registrySummary` / `testRegistryConnection`（実装済み） |
| `POST /admin/migrate/test`, `/migrate/preview` | **B3** | `Query.testSourceConnection` / `previewSource`（実装済み） |
| `POST /admin/disaster-email-backup/verify` | **B2** | `Mutation.verifyDisasterBackup`（新規） |
| `/raft/append`, `/raft/vote` | **B4** | 対象外 |
| `/closed-timestamp/receive`, `/publish` | **B4** | 対象外 |

**すでに撤廃済み**: `object-table`（B2/B3）、`keys/status`・`keys/revoke`（B3/B2）。

---

## 5. `aruaru.yaml`（B1 の宣言的設定）スキーマ案

```yaml
version: "1"

# --- 静的（プロセス再起動が必要なもの。ホットリロード対象外） ---
server:
  data_dir: "./data"
  pg_port: 5432
  graphql_port: 4000
  metrics_addr: "127.0.0.1:9090"
  log_level: "info"
  tls:
    cert: "${ARUARU_TLS_CERT}"
    key:  "${ARUARU_TLS_KEY}"

raft:
  node_id: 1
  role: "voter"           # voter | learner
  peers: []               # ["2@host:5433", ...]
  learner_peers: []

# --- 動的（ホットリロード対象。書き換えると reconcile() が走る） ---
query:
  parallel:
    enabled: false
    max_workers: 4
    chunk_size: 10000
    strategy: "hash"      # hash | range

backup:
  schedule:
    enabled: false
    cron: "0 3 * * *"
    kind: "full"

federation:
  sources: []             # [{name, kind, uri, read_only, pushdown}]

follower_read:
  target_lag_ms: 3000

wal:
  safekeepers: 3
  quorum: 2

sharded_store:
  shards: 0               # 0 = 論理コア数

disaster_backup:          # feature = "disaster_email_backup" のときのみ
  email:
    to: "${DISASTER_BACKUP_TO}"
    smtp: "${DISASTER_BACKUP_SMTP}"

# --- コントロールプレーン（RPoem）からの動的 config 取得 ---
control_plane:
  execution_config:
    # file: "/etc/aruaru/execution-config.json"
    poll:
      url: "http://localhost:3100/aruaru/execution-config"
      interval_ms: 10000
  graph_api_token: "${ARUARU_GRAPH_TOKEN}"

watch_config:
  enabled: true
  interval_ms: 2000
  startup_delay_ms: 500
```

`CLI フラグ` は「`--config <path>` と、設定ファイルを上書きする少数の
デバッグ用フラグ」だけに縮小する（Cosmo と同じ考え方）。

---

## 6. ホットリロード機構（新規モジュール `aruaru-server::config`）

```
config/
  mod.rs        AruaruConfig（serde）、load()、`${VAR}` 展開
  watch.rs      notify::RecommendedWatcher + SIGHUP ハンドラ
  reconcile.rs  差分適用: 新 config を AppState の各 Arc<Mutex<..>> へ反映
```

- **静的セクション**（`server`, `raft`）の変更は「要再起動」を warn ログに
  出すだけ（Cosmo と同じ制約）。
- **動的セクション**は `reconcile(old, new, &app_state)` が
  フィールドごとに差分を検出して `*handle.lock() = ...` する。
- `execution_config` は別タスクが `poll.interval_ms` ごとに取得し、
  ハッシュが変わったらエンジンへ再適用。

---

## 7. クライアント移行（`/admin/*` を消す以上、必須）

| クライアント | 現状 | 移行後 |
|---|---|---|
| `admin/src-tauri`（Tauri 管理 GUI） | `admin_get`/`admin_post` で `/admin/*` を直叩き | (a) 設定編集タブ = `aruaru.yaml` をローカル編集して保存（またはコントロールプレーン経由 push）、(b) アクション/観測タブ = GraphQL クライアントに置換（`/graphql` へ POST） |
| `android/.../MainActivity.kt` | `GET /admin/cluster` | GraphQL `query { clusterStatus { ... } }` |
| `web/`（リバースプロキシ） | `/admin/*` をプロキシ | `/graphql` のみプロキシ（管理者判定は据え置き） |
| `AruaruClient.java` / `client.py` | 要調査 | 同上 |

移行は各バケツの実装と**同一 PR/コミットで**行う（`/admin` ルート削除と
クライアント更新がズレると壊れる時間帯が生まれる）。

---

## 8. フェーズ（「一気に」でも作業はこの順で積む）

- **P0（本文書）** 設計思想・4 バケツ仕分け・目標 HTTP 面の確定。 ← **済**
- **P1 宣言的設定基盤** `aruaru-server::config`（load + watch + reconcile）、
  `aruaru.yaml`、`--config` フラグ。既存 CLI フラグは互換維持。 ← **済（2026-08-29 続き6）**
  - `crates/aruaru-server/src/config/{mod,reconcile,watch}.rs` 新設。
  - 依存追加は `serde_norway` のみ。ファイル監視は mtime ポーリング
    ＋ `SIGHUP`（`cfg(unix)`）を自前実装（`notify` 不使用）。
  - P1 で reconcile 接続済み: `backup.schedule`・`federation.sources`。
    残り（`query.parallel` 等）は P2。
  - `aruaru.example.yaml` を同梱。`cargo test -p aruaru-server` 失敗 0。
- **P2 B1 の移送** ← **一部済（2026-08-29 続き7）**
  - `query.parallel`（4フィールド化）: `AdminState.parallel` を
    `admin_shared::ParallelConfigState` へ統一。`reconcile` 接続。
    GraphQL `parallelConfig` query を実データ化、`setParallelConfig`
    mutation と `ParallelConfigInput` を撤廃。REST `GET/POST /admin/parallel`
    と `get_parallel`/`set_parallel` ハンドラと旧7フィールド `ParallelConfig`
    構造体を削除。Tauri `get_parallel_config` を GraphQL へ、
    `set_parallel_config` は「aruaru.yaml で管理」を返す形に。
  - `follower_read.target_lag_ms`: `ClosedTimestampCoordinator` /
    `ClosedTimestampTracker` の `target_lag_nanos` を `Arc<AtomicU64>` 共有に
    変更（`set_target_lag_nanos`）。`reconcile` 接続。全 tracker へ即反映。
  - `wal` / `sharded_store`: 静的扱いと確定（`restart_required`）。
  - **P2 残り**: `disaster_backup.email` の reconcile 接続、
    `/admin/parallel/explain`・`/admin/parallel/jobs` の GraphQL 実データ化 →
    REST ルート撤廃（GraphQL `explainDistributed`・`parallelJobs` は現状
    スタブ。実ロジックの移植が必要）。Tauri の設定タブ全体を
    `aruaru.yaml` 編集 UI に。
- **P3 B2/B3 の残り** ephemeral-query / multi-raft / sharded-store put/get /
  closed-timestamp / wal-service を GraphQL query/mutation 化。対応 `/admin`
  ルート削除。Tauri/Android/web を GraphQL クライアントへ。
- **P4 `/admin` ルーター自体を削除** `admin::admin_routes` を撤去。
  `admin.rs` を「GraphQL リゾルバが使うヘルパー」だけに縮小 or 解体。
- **P5 コントロールプレーン** RPoem に execution-config 配信
  （`open-runo-schema-registry`）、feature フラグ配信、aruaru-server 側の
  ポーリング取得を実装。Cosmo の CDN/Control Plane 相当を OSS で。
- **P6 ドキュメント整合** `cosmo/README.md`、各言語 README、`PORTING.md`、
  RPoem 側 CLAUDE.md の同期。

各フェーズ完了時に `cargo test --workspace` 失敗 0 を確認し、CLAUDE.md 冒頭の
「🔄 セッション再開用メモ」を更新して push（リミット接近前に必ず）。

---

## 9. 影響を受けるリポジトリ

| リポジトリ | 影響 |
|---|---|
| `aruaru-db` | `aruaru-server`（config 基盤新設、`admin.rs` 解体）、`aruaru-graphql`（query/mutation 拡充）、`admin/src-tauri`（GraphQL クライアント化）、`android`、`web`、`cosmo/`、docs |
| `RPoem` | `open-runo-schema-registry`（execution-config 配信）、`open-runo-feature-flags`（配信）、`open-runo-federation`（合成）、CLAUDE.md 同期エントリ |
| `open-raid-z` | 正本ルールの参照のみ（変更なし想定） |

---

## 10. 却下した代替案

- **「REST を 1 本ずつ GraphQL mutation へ」だけ続ける** → §0 の理由で却下
  （アンチパターンの移送）。ただし B2/B3 に限れば正しいので P3 で採用。
- **`/admin/*` を gRPC(ConnectRPC) に置換** → プロトコルが変わるだけで
  「実行時ミューテーションで設定管理」構造は不変。却下。
- **設定も GraphQL Subscription で配信** → 面白いが、Cosmo は素直に
  ポーリング。まずは Cosmo 準拠（P5）。Subscription 化は将来検討。

---

## 付録 A. 実在する「CockroachDB × Snowflake ハイブリッド変種」の調査(2026-08-29)

ユーザー指示: 「aruaru-db は CockroachDB と Snowflake の良い所取りのハイブリッドの
特殊な変種の実在する DATABASE の実装理論や技術を取り入れて」。英日で Google /
GitHub 調査した結果。

### A.1 該当する実在システムと、その要素技術

| システム | Cockroach 側(強整合 OLTP) | Snowflake 側(分離・列指向 OLAP) | 橋渡しの要素技術 |
|---|---|---|---|
| **TiDB / TiKV + TiFlash**(PingCAP、VLDB 2020「TiDB: A Raft-based HTAP Database」)| TiKV = Multi-Raft、Region 単位、線形化可能 | **TiFlash** = 列指向レプリカ、独立スケール、`DeltaTree` エンジン | **Raft Learner** が行→列変換しながら非同期リプレイ。読み取り時に **Raft index + MVCC** で Snapshot Isolation を検証 |
| **CockroachDB** 本体 | Range + closed timestamp + follower read + bounded staleness | ベクトル化実行、`Data Boost`(分離コンピュート)| closed timestamp(= aruaru-db が既に実装) |
| **SingleStore** | 分散、行ストア | 列ストア、`Bottomless`(S3 無制限ストレージ、compute/storage 分離)| 単一エンジンで行↔列を統合(レプリカ非同期) |
| **Neon** | Postgres 互換 | **safekeeper / pageserver 分離**(WAL サービス化)| = aruaru-db `wal_service` が既に借用 |
| **Databend** | — | Snowflake 型、オブジェクトストレージ直結、Rust | = aruaru-db `table_format`(object-table)が既に借用 |
| **RisingWave** | — | ストリーミング SQL、S3 ステートバックエンド、Rust | — |

**「特殊な変種」の代表格は TiDB**(Raft 強整合 OLTP + 列指向 Raft-Learner レプリカで
1 システム HTAP)。CockroachDB の「Raft・follower read」と Snowflake の
「独立スケールする列指向解析ストア」を一体化している。

### A.2 aruaru-db が既に取り込んでいるもの

- CockroachDB: closed timestamp / follower read / bounded staleness
  (`crates/aruaru-dist/src/closed_ts.rs`)、Multi-Raft(`multi_raft.rs`)、
  Serverless の ephemeral SQL pod(`crates/aruaru-server/src/ephemeral_pod.rs`)
- Snowflake 系: Neon 型 disaggregated storage(`wal_service`)、Databend 型
  object-table(`table_format`)、ScyllaDB shard-per-core(`sharded_store`)

### A.3 未取り込みで、取り入れる価値があるもの(将来フェーズ)

1. **Raft-Learner 列指向レプリカ**(TiFlash 型)。`--raft-role learner` は既に
   あるが、Learner 上で **行→列変換**して独立の解析ストアを作る部分が無い。
   → RPoem 配信の execution-config で「どのテーブルに列レプリカを何個持つか」
   を宣言(TiDB の `ALTER TABLE ... SET TIFLASH REPLICA n` 相当)。P5〜。
2. **読み取り時の Raft index + MVCC による Snapshot Isolation 検証**。
   今の closed timestamp(P2 でホットリロード化済み)と組み合わせ、Learner
   レプリカからの一貫読み取りの根拠を厳密化する。
3. **DeltaTree 型の更新耐性のある列エンジン**(頻繁な更新 + 高速スキャン両立)。
   現状の object-table(不変セグメント + 時間旅行)に delta 層を足す方向。

これらは本再設計(管理面の GraphQL 一本化)とは別トラックだが、**execution-config
の配信内容**(どのテーブルを列レプリカ化するか等)として管理面に載るため、
P5(コントロールプレーン)で接点を持つ。`docs/HYBRID_NETWORK_ARCHITECTURE.md`・
`README-English.md` の HTAP 記述とも整合を取ること。
