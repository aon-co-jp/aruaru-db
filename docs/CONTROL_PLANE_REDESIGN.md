# 管理面の再設計 — 「REST API 不要」を抜本的に実現する

> ステータス: **実装中（2026-08-29 起票 / 2026-08-31 付録 A を 2026 最新設計として大幅拡充）— P0 設計確定 / P1 完了 / P2 主要部完了 / P3 本体着手（closed-timestamp・wal-service・sharded-store を GraphQL 化・REST 撤廃、ephemeral-query・multi-raft は設計メモを残し次スライスへ）**
> 対象: `aruaru-db` + `RPoem`（SET）
> 決定者: masahiro ishizuka（AON CEO）／ 起案: セッション横断作業
> 関連: [`CLAUDE.md` 冒頭「🎯 最重要・最優先」](../CLAUDE.md) ・ [`PORTING.md`](../PORTING.md)
> 進捗の詳細は `CLAUDE.md` 冒頭「🔄 セッション再開用メモ」と HANDOFF（続き5〜）。

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

## 2. 新・設計哲学（この再設計の土台）

### 2.0 一文で言うと

> **すべては「望ましい状態（desired state）の宣言」と「それへ収束させる
> reconciliation」で表す。データプレーンに命令的 RPC を置かない。**

管理設定も、APIキーも、HTAP の列レプリカ配置も、同じ 1 つの考え方で扱う。
「今こうしろ」（命令）ではなく「あるべき姿はこれ」（宣言）を書き、
システムが差分を埋める。これは以下 4 つの実在システム群の**共通解**であり、
本設計はそれを 1 つのエコシステム（aruaru-db + RPoem）に統合する。

| 源流 | 借りる考え方 | 一次資料 |
|---|---|---|
| **Kubernetes / GitOps（Argo CD・Flux）** | 宣言的 state + 連続 reconciliation + 外部の不変な source of truth + ドリフト自動補正（self-healing） | [ArgoCD reconciliation](https://argo-cd.readthedocs.io/en/stable/operator-manual/reconcile/)、[GitOps principles](https://www.plural.sh/blog/what-is-gitops/) |
| **WunderGraph Cosmo / Apollo GraphOS** | データプレーン（Router＝実行・可用性最優先）と コントロールプレーン（governance・設定配信）の分離。クライアントはコントロールプレーンに触れない | [Cosmo overview](https://cosmo-docs.wundergraph.com/overview)、[Cosmo router config](https://cosmo-docs.wundergraph.com/router/configuration) |
| **TiDB / TiKV + TiFlash（VLDB 2020）** | Raft 強整合 OLTP と 列指向 OLAP を「Raft-Learner レプリカ」で 1 システム化、独立スケール。読み取り時に Raft index + MVCC で Snapshot Isolation を検証 | [TiDB: A Raft-based HTAP Database](https://www.vldb.org/pvldb/vol13/p3072-huang.pdf) |
| **SPIFFE / SPIRE（NIST SP 800-207 Zero Trust）** | 「Attest → Issue → Authenticate」の自動ループ。短命クレデンシャルの自動ローテーション、孤児クレデンシャルの自動失効、静的シークレット・ゼロ | [SPIFFE](https://www.paloaltonetworks.com/cyberpedia/what-is-spiffe)、[CockroachLabs: SPIFFE/SPIRE で zero-trust DB 認証](https://www.cockroachlabs.com/blog/zero-trust-database-authentication-spiffe-spire/) |

### 2.1 原則（12 か条）

**A. プレーン分離**

1. **データプレーンは GraphQL に徹する。REST API は例外なく完全撤廃。**
   最終形で `aruaru-server` が公開する HTTP は `/graphql`・`/graphql/sdl` と、
   ops 面の `/health*`・`/metrics`（k8s・Prometheus 規約であって CRUD の
   REST API ではない、という明示的 carve-out）だけ。
   `/admin/*` は 1 本も残さない。`/v1/keys/self-issue` も GraphQL mutation へ
   移す。ノード間 `/raft/*`・side transport はバイナリ化する（§3）。
   データプレーンは「可用性最優先で、コントロールプレーンが落ちても
   動き続ける」（Cosmo Router と同じ立場）。

2. **コントロールプレーンは RPoem。**
   `open-runo-schema-registry`（execution-config 配信＝Cosmo の CDN 相当）・
   `open-runo-feature-flags`・`open-runo-federation`（合成）・
   `open-runo-scim` / `open-runo-security`（SCIM 2.0 / OIDC＝Cosmo
   Enterprise 相当を OSS で）。`aruaru-server` はここから設定を
   **取得する側**。クライアントはコントロールプレーンに直接触れない。

3. **ノード間 RPC は管理面でも設定面でもない。GraphQL 化せず、バイナリ化する。**
   `/raft/append`・`/raft/vote`・`/closed-timestamp/receive|publish` は
   クラスタ内部トランスポート。GraphQL には載せない。ただし HTTP のままにも
   しない——`crates/aruaru-dist/src/raft/binary_transport.rs` へ寄せるのが
   確定ゴール（REST 完全撤廃の一部。P4 以降の必須項目）。

**B. 宣言と reconciliation（設定）**

4. **運用設定は宣言的ドキュメント。実行時ミューテーションではない。**
   `aruaru.yaml`（このノード固有・静的寄り）＋ RPoem 配信の
   `execution-config`（クラスタ全体・動的）。**`setX` mutation は作らない。**
   設定変更は「ドキュメントを書き換える → reconcile が走る → 収束」。

5. **source of truth は外部・不変・バージョン管理下（GitOps 原則）。**
   `aruaru.yaml` は Git 管理される。稼働中プロセスの状態は
   source of truth ではなく、その**射影**にすぎない。

6. **reconcile は冪等で、差分だけを当てる。**
   同じ config を何度当てても同じ結果。変わっていない項目は触らない。
   （実装: `config::reconcile(new, previous, &state) -> ReconcileReport`）

7. **ホットリロードは最初から作り込む。ドリフトは警告する。**
   mtime ポーリング（Cosmo `watch_config` と同方式）＋ `SIGHUP`。
   静的セクション（`server`/`raft`/`wal`/`sharded_store`）の変更は
   「要再起動」を warn ログ＋`restart_required` に記録するだけ
   （進行中状態を失わないための意図的な制約。Cosmo の静的セクションと同じ）。

8. **壊れた設定を保存しても稼働中インスタンスは無事。**
   YAML 解析エラー時は error ログを出して**直前の設定を維持**。

**C. アクションと観測（GraphQL）**

9. **「一度きりの副作用」だけが GraphQL Mutation。**
   `createBackup`・`runMigration`・`rebalanceCluster`・`clusterPropose`・
   `objectTableCommit` … は冪等でない＝正当な RPC。subgraph の `Mutation`
   に置く（VersionlessAPI の一部）。「設定の書き込み」はここに**含めない**。

10. **観測は GraphQL Query ＋ Prometheus。**
    `clusterStatus`・`keyStatus`・`parallelConfig`（実効値）・
    `objectTable` 履歴 … はグラフに載せる。数値カウンタは `/metrics` にも。

**D. クレデンシャルも「宣言 + 自動ループ」（SPIFFE 哲学）**

11. **APIキーは自動ライフサイクル。人間の承認キューを置かない。**
    - 自動発行: `POST /v1/keys/self-issue`（認証不要＝「即発行できること
      自体が承認手続き」）が `viewer` ロール・短命（既定 24h）キーを出す。
    - 自動承認: 人手の approve ステップは存在しない。
    - 自動破棄: GraphQL `revokeKeys(owner)` mutation（アクション）。
    - 自動削除: 期限切れは `verify()` 実行時に検知してその場で除去。
    正本の設計・実装は `crates/aruaru-dist/src/keyring.rs`（`KeyGuardian`）、
    RPoem 側は `open-runo-router::keyring` が同じ設計を独立実装（Cargo 依存は
    結合しない既存方針）。SPIFFE の「短命・自動ローテーション・孤児の自動
    失効・静的シークレット ゼロ」を、mTLS/SVID の重装備なしに最小構成で。

**E. HTAP ハイブリッドも「宣言」で（TiDB 哲学）**

12. **列レプリカ配置・follower read の許容ラグ等も宣言的設定。**
    「どのテーブルに列指向 Learner レプリカを何個持つか」は execution-config
    に書く（TiDB の `ALTER TABLE … SET TIFLASH REPLICA n` を宣言化した相当）。
    `follower_read.target_lag_ms`（P2 実装済み・完全ホットリロード）も同じ。
    強整合 OLTP（Raft）と 列指向 OLAP（Learner）の**両立の度合い**を、
    命令ではなく「望ましい姿」として表す。理論的裏付けは付録 A。

### 2.2 判断フロー（新しいエンドポイント／設定を足すとき）

```
その項目は「望ましい状態」か？（何度書いても同じ結果か）
├─ YES → 宣言的設定（aruaru.yaml / execution-config）へ。setX mutation は作らない。
│         └─ 稼働中に無停止で変えられるか？
│             ├─ YES → reconcile でホットリロード
│             └─ NO  → 静的扱い。reconcile は restart_required を報告するのみ
└─ NO（冪等でない副作用）
    ├─ 状態の読み取りだけ → GraphQL Query（＋ /metrics）
    ├─ 一度きりのアクション → GraphQL Mutation
    └─ クラスタ内部通信   → 内部トランスポート（GraphQL 化しない）

いずれの場合も着手前に自問:
「これは aruaru-db + RPoem SET の価値（SCIM/SSO 相当・APIキー自動管理・
 VersionlessAPI 互換）を強化するか？」→ NO なら、やらない。
```

---

## 3. 目標 HTTP 面（再設計後）

> **REST API 完全撤廃の厳命（2026-08-29、ユーザーが「肝に命じて」と明言）**:
> SET（aruaru-db + RPoem）の**全体から REST API を例外なく完全撤廃**する。
> 下表で「維持」に見えるものも、CRUD の REST API ではないことを毎回明示的に
> 述べられる場合のみ残す（health / metrics = k8s・Prometheus 標準の ops 面）。
> それ以外はすべて GraphQL かバイナリトランスポートへ寄せる。

### `aruaru-server`（データプレーン）— 最終形

```
/graphql              GraphQL（query = 観測 / mutation = アクション / subscription = 将来）
/graphql/sdl          Federation SDL（wgc / RPoem federation が取得）
/health /health/ready /health/live   ← ops 面。k8s プローブ規約。REST API ではない（明示的 carve-out）
/metrics              ← ops 面。Prometheus 規約。別リスナ 127.0.0.1:9090。REST API ではない（明示的 carve-out）
```

**撤廃対象（現状は HTTP、最終形では消える）**:
- `/admin/*`（約40本）… §4 の 4 バケツで GraphQL / 宣言的設定へ（P2〜P4）。
- `/v1/keys/self-issue` … **GraphQL mutation `selfIssueKey`（認証ガード無し）へ移す**。
  「認証不要で即発行できること自体が承認手続き」という性質は mutation でも保てる（P3）。
- `/raft/append`・`/raft/vote`・`/closed-timestamp/receive|publish` …
  ノード間 RPC。**バイナリトランスポート化が確定ゴール**（「いつか」ではなく
  P4 以降の必須項目）。`crates/aruaru-dist/src/raft/binary_transport.rs` が既にある。

### RPoem（コントロールプレーン）
```
execution-config の配信（ポーリング or push）      ← open-runo-schema-registry
feature フラグの配信                               ← open-runo-feature-flags
subgraph 合成（Federation composition）            ← open-runo-federation
SCIM 2.0 / SSO(OIDC)（Cosmo Enterprise 相当・OSS） ← open-runo-scim / open-runo-security
APIキー・ライフサイクル（KeyGuardian）             ← open-runo-router::keyring（aruaru-db と設計共有）
```
RPoem 側も同じ厳命。`open-runo-router` の REST ハンドラ（`handlers_hyper.rs`・
`openapi.rs` 等）は GraphQL / 宣言的設定へ寄せる（別途 RPoem 側 CLAUDE.md で追跡）。

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
| `POST /v1/keys/self-issue` | **B2** | `Mutation.selfIssueKey`（認証ガード無し・P3。「即発行＝承認」を mutation で保つ） |
| `/raft/append`, `/raft/vote` | **B4** | バイナリトランスポート化（`raft/binary_transport.rs`、P4 以降の必須） |
| `/closed-timestamp/receive`, `/publish` | **B4** | 同上（side transport のバイナリ化） |

**すでに撤廃済み**: `object-table`（B2/B3）、`keys/status`・`keys/revoke`（B3/B2）、
`GET/POST /admin/parallel`（B1 → `aruaru.yaml: query.parallel`、P2）。

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
  - **P2 残り（続き8 で一部消化）**:
    - ✅ `/admin/parallel/explain`・`/admin/parallel/jobs` → 撤廃。
      GraphQL `explainDistributed` query（実ロジック移植・`AdminMutation`
      から `AdminQuery` へ移設）と `parallelJobs` query に一本化。Tauri
      2 コマンドを GraphQL へ。→ **`/admin/parallel*` は完全撤廃**。
    - ✅ `/v1/keys/self-issue` → `Mutation.selfIssueKey`（認証ガード無し、
      `VcsMutation`）へ移設。`build_schema` が `AdminCtx.keyring` と同一
      `KeyGuardian` を schema data へ注入。REST ルート・ハンドラ削除。
    - ⏳ `disaster_backup.email` reconcile 接続: config スキーマは
      `EmailBackupTargetConfig` と同じ 7 フィールドへ拡張済み。reconcile
      本体は `feature = "disaster_email_backup"` ゲート + `replicator`
      注入の要否判断 + feature ゲート付きテストが必要な**別スライス**。
    - ⏳ Tauri 設定タブ全体の `aruaru.yaml` 編集 UI 化。
- **P3 B2/B3 の残り** — ✅ **本体着手・主要 3 群完了（2026-08-31、続き10）**:
  - ✅ `closed-timestamp`: `Query.closedTimestamp`（status）・
    `Query.planFollowerRead`（`table` 指定で `select_follower_read` 実データ
    読み出しまで）、`Mutation.closedTsRegisterRange`・`Mutation.closedTsAdvance`。
    旧 REST `GET /admin/closed-timestamp`・`/range`・`/advance`・`/plan` を
    `admin.rs` から削除。`AdminCtx.closed_ts`（`closed_ts_coordinator()`）注入。
    **`/receive`・`/publish` は B4**（ノード間 side transport、既に
    `binary_transport.rs` のバイナリ経路。管理トリガーの `/publish` REST は
    「いつ・誰に配布するか」を人間が指示する制御面のため残置、P4 で再検討）。
  - ✅ `wal-service`: `Query.walService`（status）・`Query.walPage`
    （`get_page_at_lsn` = 読み取りなので Query。§4 表では B2 と記載していたが
    §2.2 判断フローに従い Query へ是正）、`Mutation.walAppend`・
    `Mutation.walCreateImageLayer`（compaction）。旧 REST `GET /admin/wal-service`
    ・`/append`・`/page`・`/image-layer` を削除。`AdminCtx.wal_storage` 注入。
    status の safekeeper 列挙を `0..n`→`1..=n` に是正（REST 版は先頭を
    取りこぼしていた）。
  - ✅ `sharded-store`: `Query.shardedStoreGet`・`Query.shardedStoreStats`、
    `Mutation.shardedStorePut`。旧 REST `POST /admin/sharded-store`・
    `GET /admin/sharded-store/:key`・`GET /admin/sharded-store-stats` を削除。
    `AdminCtx.sharded_store` 注入（`AdminState.sharded_store` を `Arc` 化）。
    mpsc ブロッキング recv は `tokio::task::spawn_blocking` で退避（REST と同じ）。
  - ✅ `disaster_backup.email` の config スキーマは 7 フィールド拡張済み
    （続き8）。reconcile 本体は feature ゲート付きの別スライスで保留（変更なし）。
  - ⏳ **`ephemeral-query` / `multi-raft` は次スライスへ（技術的理由を明記）**:
    - `ephemeral-query`: `run_ephemeral_query` は `std::env::current_exe()` +
      `tokio::process::Command` で自分自身を `--ephemeral-worker` 再起動する
      **`aruaru-server` バイナリ固有処理**。`ephemeral_pod` モジュールは
      lib クレートに無く `aruaru-graphql` から参照不能。`AdminCtx` へ
      `Arc<dyn EphemeralRunner>` trait を注入し `aruaru-server` 側で実装する
      **trait 化リファクタが必要**。状態注入だけで済む上記 3 群とは規模が違う
      ため分離した（半端な足場を成果と呼ばない原則）。
    - `multi-raft`: `MultiRaftCluster<A>` の `A` = `crate::cluster::EngineApplier`
      が `aruaru-server` ローカル。`split` は `applier: A` を要求し、`AdminCtx`
      から具体型を名指しできない。`Arc<dyn MultiRaftHandle>` trait object 化
      **または** `EngineApplier` を `aruaru-dist` へ移設する必要がある。
      同上の理由で次スライスへ。
  - **クレート境界の注意（続き8 調査、確認済み）**: `aruaru-graphql` の
    リゾルバは `aruaru-server` の `mod` を参照できない。状態が
    `aruaru-dist`/`aruaru-query` 型なら `object_table`/`keyring`/`topology` と
    同じ `AdminCtx` 注入で済む（= 上記 3 群）。`aruaru-server` 固有の
    プロセス/ジェネリック処理は trait 注入が要る（= ephemeral / multi-raft）。
  - **Tauri/Android/web クライアント**: 上記 3 群の旧 REST は grep で
    Tauri/Android/web からの参照が無いことを確認済み（`object-table`/`keys` と
    同様に安全に撤廃）。`cluster`/`backup/schedule`/`federation` の撤廃は
    引き続きクライアント移行待ち（続き4 の記載どおり）。
- **P4 `/admin` ルーター＋残る非 GraphQL HTTP を撤去**
  `admin::admin_routes` を撤去、`admin.rs` を GraphQL リゾルバのヘルパーだけに。
  **ノード間 `/raft/*`・side transport（`/closed-timestamp/receive|publish`）を
  `raft/binary_transport.rs` のバイナリ経路へ寄せ、HTTP リスナから外す。**
  → データプレーンの HTTP は `/graphql`・`/graphql/sdl`・`/health*`・`/metrics`
  だけになる（REST 完全撤廃の到達点）。
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

## 付録 A. 「CockroachDB × Snowflake ハイブリッド変種」の実装技術調査
### — 2026 年時点の最新設計として再構成(2026-08-31 大幅拡充)

ユーザー指示(要求③、2026-08-31 再強調):「aruaru-db は CockroachDB と
Snowflake の良い所取りのハイブリッドの特殊な変種の実在する DATABASE の
実装理論や技術を取り入れて。TiDB だけに限らず**関連する全て**を、世界中の
言語で Google と GitHub を検索し、**実装方法(アーキテクチャ設計・データ
構造・アルゴリズム)まで**調べ、設計文書を 2026 年時点の最新設計として
再設計せよ」。英・日・独で Google / GitHub / 一次論文を再調査した結果を、
**どの技術のどの部分を aruaru-db がどう取り込むか(取り込まない判断は
その理由も)**の形で整理する。

> **この付録の位置づけ**: 本文書の主題(管理面の REST 完全撤廃・宣言的
> コントロールプレーン)とは別トラックの「データプレーン設計の指針」だが、
> 「どのテーブルを列レプリカ化するか」「follower read の許容ラグ」等は
> **execution-config / `aruaru.yaml` の宣言的設定として管理面に載る**ため、
> §2 原則 12・§5 スキーマ・P5(コントロールプレーン)と直結する。
> `docs/HYBRID_NETWORK_ARCHITECTURE.md`・`README-English.md` の HTAP 記述
> とも整合を取ること。

---

### A.0 「特殊な変種」を一文で定義する(2026 再設計の結論)

> **aruaru-db =「Raft 強整合 OLTP(CockroachDB 系)＋ Raft-Learner 非同期
> 列レプリカ(TiDB/TiFlash 系)＋ 不変スナップショット・オブジェクト
> テーブル(Snowflake / Iceberg / Databend 系)＋ WAL/ページ分離
> (Neon 系)＋ Git-on-SQL 時間旅行(独自)」を単一 Pure Rust プロセスに
> 統合した HTAP データベース。**

「良い所取り」の実体は次の対応表(2026 時点):

| 取り込む性質 | 出典系統 | aruaru-db での実装(crate) | 状態 |
|---|---|---|---|
| Range 単位の独立 Raft グループ(Multi-Raft) | CockroachDB / TiKV | `aruaru-dist/src/multi_raft.rs`, `shard/topology.rs` | 実装済(単一プロセス) |
| closed timestamp + follower read + bounded staleness | CockroachDB / TiKV safe-ts / YugabyteDB | `aruaru-dist/src/closed_ts.rs` | 実装済(P2 でホットリロード化) |
| HLC(Hybrid Logical Clock)による版付け | Spanner / CockroachDB / YugabyteDB | `aruaru-dist/src/hlc.rs`(`now`/`update`、CAS実装) | 実装済(2026-08-31、既存`closed_ts`等への配線は次段階) |
| Raft-Learner 上での 行→列 非同期変換レプリカ | TiDB/TiFlash | `aruaru-dist/src/columnar_applier.rs`(`Applier`実装)+ `aruaru-server --columnar-learner`(実プロセス、binary Raft経由で実複製) | 実装済(2026-08-31、2プロセス間の実HTTP/実TCPで検証済み。真のdelta蓄積はA.6-4/次段階) |
| 読み取り時の Raft index + MVCC による SI 検証 | TiDB/TiFlash | closed_ts の gate はあるが Raft index 突合は無い | 未取込(A.6-3) |
| DeltaTree(B+木 × LSM)= 更新耐性のある列エンジン | TiDB/TiFlash | object-table は不変セグメントのみ、delta 層無し | 未取込(A.6-4) |
| 不変マイクロパーティション + メタデータ pruning + time travel | Snowflake / Iceberg / Delta / Databend | `aruaru-backup/src/table_format.rs`(snapshot→segment→block 3層、min/max + bloom) | 実装済 |
| WAL サービス(quorum 耐久化)と pageserver(ページ再構成)の分離 | Neon(> Aurora) | `aruaru-dist/src/wal_service.rs`(term fencing + `flushLsn[n-quorum]`) | 実装済(単一プロセス) |
| セグメント単位のゾーンマップ / スパース索引 | ClickHouse MergeTree / DuckDB row group / SingleStore segment | `aruaru-query/src/olap.rs`(`SegmentStats`、既定 1024 行) | 実装済(min/max のみ) |
| 型認識軽量圧縮(辞書 / RLE / FSST / ALP / bitpack) | DuckDB / Parquet / TiFlash(LZ4) | `olap.rs` の適応的辞書エンコード(カーディナリティ閾値 0.7) | 部分(辞書のみ) |
| ステートレス計算ノード + 共有メタデータ(Keeper 相当) | ClickHouse SharedMergeTree / Snowflake / TiDB Serverless | `ephemeral_pod.rs`(使い捨てプロセス)、`table_format` MetaService(CAS) | 部分 |
| shard-per-core(shared-nothing、メッセージパッシング) | ScyllaDB / Seastar | `aruaru-query/src/sharded_store.rs`(murmur3 ルーティング) | 実装済(独立ストア) |
| 決定的シミュレーションテスト(DST) | FoundationDB | `aruaru-dist/src/raft/sim.rs` | 実装済 |

---

### A.1 各システムの実装方法(アーキテクチャ・データ構造・アルゴリズム)

#### A.1.1 TiDB / TiKV + TiFlash — Raft-based HTAP(**本命の参照実装**)

- **論文**: VLDB 2020「TiDB: A Raft-based HTAP Database」
  <https://www.vldb.org/pvldb/vol13/p3072-huang.pdf> /
  <https://dl.acm.org/doi/10.14778/3415478.3415535>
- **TiFlash の 2 コンポーネント構成**: (a) 列指向ストレージ本体、(b) TiFlash
  proxy(TiKV ベースの Multi-Raft フレームワークを FFI で export する
  Cライブラリ)。proxy が apply 結果(region meta 含む)を FFI 経由で
  TiFlash へ渡し RSM(Replicated State Machine)を直接維持させる**プッシュ型**。
  <https://docs.pingcap.com/tidbcloud/tiflash-overview/>
- **Learner ロール**: TiFlash は Raft グループの **learner(非投票)** として
  参加。DML は TiFlash の ack を待たない(書き込みレイテンシに影響しない)。
  列レプリカは Raft Learner consensus で非同期複製され、**読み取り時に
  Raft index + MVCC で Snapshot Isolation を検証**する。
- **DeltaTree エンジン**: 「B+木 と LSM木 のハイブリッド」。2 空間:
  - **stable 空間**: パーティションデータを chunk として列ごとに格納
    (Parquet 類似フォーマット、LZ4 圧縮)。読み出し最適化。
  - **delta 空間**: TiKV が生成した順序のまま追記(linearizability 維持)。
    書き込み最適化。後でバッチ変換して stable へマージ(compaction)。
  <http://muratbuffalo.blogspot.com/2023/10/tidb-raft-based-htap-database.html>
- **宣言的配置**: `ALTER TABLE ... SET TIFLASH REPLICA n` で「どのテーブルに
  列レプリカを何個持つか」を宣言 → aruaru-db では execution-config へ
  (§5・P5、A.7)。

#### A.1.2 CockroachDB — 分散 SQL の強整合 OLTP 基盤

- **Storage Layer**: <https://www.cockroachlabs.com/docs/stable/architecture/storage-layer>
- **Range**: キー空間を ~64MiB の連続チャンク(range)へ分割。各 range が
  独立した **Raft グループ**、3 or 5 レプリカ。負荷で自動 split/merge。
- **MVCC**: **HLC タイムスタンプ**で版を区別。更新は上書きせず高タイム
  スタンプの新版を作る。GC 期限も HLC で管理。
- **Pebble**(Go 製 KV エンジン、RocksDB 由来を CockroachDB のアクセス
  パターンへ最適化)。MVCC range tombstone を **Pebble range key** として
  格納し、Raft range 境界で fragmentation(物理 or 論理)。
- **closed timestamp / follower read / bounded staleness**: この時刻以下に
  新規書き込みは現れない保証を leaseholder が前進させ side transport で
  follower へ配布 → follower がローカルで一貫読み取り。
  = aruaru-db `closed_ts.rs` が実装済み。
- **protected timestamp**: GC より前の時刻を「保護」してバックアップ/
  CDC の読み取り基点を保証。<https://www.cockroachlabs.com/blog/protected-timestamps-for-less-garbage/>
  → aruaru-db の Git-on-SQL コミットが実質同じ役割(コミットが指す
  root_hash 配下の Prolly ノードは参照が切れるまで生存)。

#### A.1.3 YugabyteDB — DocDB(RocksDB + Raft + HLC)

- **DocDB**: 高度カスタマイズした RocksDB の上に **クラスタ全体の MVCC** を
  HLC で構築。<https://docs.yugabyte.com/stable/architecture/transactions/>
- **書き込みパス**: leader が batch を Raft ログへ append → **HLC で
  timestamp を選ぶ** → Raft 複製 → callback 後にローカル DocDB へ apply。
- **分散トランザクション**: 対象タブレット群へ **provisional record** を
  別 RocksDB インスタンス(`IntentsDB`)に書き、commit まで読者に不可視。
  → aruaru-db は現状シングルプロセス 2PC 無しのため直接は取り込まないが、
  「intent を別ストアに隔離する」発想は将来のマルチテナント書き込みで参考。

#### A.1.4 Snowflake — ストレージ/コンピュート分離の元祖

- **3 層**: (a) storage(クラウドオブジェクトストレージ上の**不変
  マイクロパーティション**)、(b) compute(virtual warehouse)、(c) cloud
  services(認証・最適化・メタデータ)。各層が独立スケール。
- **マイクロパーティション**: 1 テーブルの行を 50–500MB(圧縮後 ~16MB)の
  列指向不変ファイルへ。**その場更新は無い**——DML は新パーティションを
  書き旧を stale マーク。
- **pruning**: クエリ述語にメタデータ範囲がマッチするパーティションだけを
  スキャン。cloud services 層がメタデータで枝刈り。
- **time travel / zero-copy clone**: ストレージ不変性の**直接の帰結**
  (別機能を上に足したのではない)。
  <https://medium.com/@krthiak/the-architecture-of-speed-how-snowflakes-micro-partitions-and-pruning-drive-query-performance-7ab5ccb087c3>
- → aruaru-db `table_format.rs`(snapshot→segment→block、min/max + bloom、
  `prev_snapshot_id` 連鎖で time travel)が既にこのモデル。Git-on-SQL
  コミットが zero-copy clone(`aruaru_branch_from`)を提供。

#### A.1.5 Neon vs Aurora — WAL 中心のストレージ分離

- **Aurora**:「log is the database」。WAL とページ処理を**単一のストレージ
  サービス**が担う(モノリシックなストレージ tier)。
- **Neon**: WAL と page service を**さらに分離**。
  - **safekeeper 群**: WAL の耐久複製だけを担当。**Paxos(Raft ではない)**
    の quorum ack で commit 確定。
    <https://jack-vanlightly.com/blog/2025/2/19/log-replication-disaggregation-survey-neon-and-multipaxos>
  - **pageserver**: WAL と data page の間に位置し、base page + committed WAL
    から**任意 LSN のページを materialize**(`get_page_at_lsn`)。
  <https://neon.com/blog/architecture-decisions-in-neon>
- → aruaru-db `wal_service.rs` が Neon 型(`Safekeeper`/`Pageserver`/
  `DisaggregatedStorage`、term fencing + `commitLSN = flushLsn[n-quorum]` +
  `get_page_at_lsn` + image layer GC cutoff)を実装済み。**Aurora 型の
  モノリシック統合は選ばない**(分離の方が follower read / ephemeral pod と
  組み合わせやすい)。

#### A.1.6 SingleStore — 単一エンジンで行 ↔ 列(Universal Storage)

- **Universal Storage(= columnstore)**: columnstore で OLTP も効率化する
  5 機能: (1) columnstore 上の **hash index**(一意制約も可)、(2) **subsegment
  access**(列ストア内の 1 行への高速アクセス)、(3) **row-level locking**、
  (4) 高選択 filter を伴う join、(5) upsert。
  <https://docs.singlestore.com/cloud/create-a-database/columnstore/universal-storage/>
- **segment**: テーブルを 100 万行チャンク(segment)へ分割。columnstore の
  各パーティションは**インメモリ rowstore segment**(直近の更新/挿入分)を
  持つ。= TiFlash の delta 空間 / ClickHouse の in-memory part と同型。
- **skiplist**(rowstore のみ)/ **hash table**(sparse bucket 配列)。
- → aruaru-db は「object-table(不変・列)」と「QueryEngine::tables
  (行・可変)」が別。SingleStore の「1 テーブル内で行 segment と列 segment
  が同居」は**未取込**(A.6-4 の delta 層と合わせて検討)。

#### A.1.7 ClickHouse — MergeTree / SharedMergeTree

- **MergeTree**: INSERT ごとに**不変 part**(ディレクトリ、ORDER BY キーで
  ソート済み)を作り、バックグラウンドで **merge**(小 part → 大 part)。
  <https://clickhouse.com/docs/engines/table-engines/mergetree-family/mergetree>
- **スパース主索引**: 行ごとではなく **granule**(既定 8192 行)ごとに
  1 mark。索引が小さく RAM に載る。merge 時に索引も merge。
  <https://clickhouse.com/docs/guides/best-practices/sparse-primary-indexes>
- **SharedMergeTree**(ClickHouse Cloud): データ + メタデータをサーバから
  完全分離、**ClickHouse Keeper** 経由で共有メタデータを read/write。
  新ノードは data part を転送せず Keeper からメタデータだけ同期 →
  2→10 ノードのスケールがほぼ即時。各サーバはメタデータのローカル
  キャッシュ + subscription で変更通知。
  <https://clickhouse.com/blog/clickhouse-cloud-boosts-performance-with-sharedmergetree-and-lightweight-updates>
- → aruaru-db `olap.rs` の `SegmentStats`(既定 1024 行)= granule 相当。
  SharedMergeTree の「Keeper = 共有メタデータの真実源、計算はステートレス」は
  **RPoem execution-config / `table_format` MetaService(CAS)**と同型 ⇒
  §2 原則 2(コントロールプレーン分離)を裏付ける独立事例。

#### A.1.8 オープンテーブルフォーマット(Iceberg / Delta Lake / Hudi / Paimon)

| | メタデータ構造 | 更新方式 | 索引 |
|---|---|---|---|
| **Iceberg** | metadata file → manifest list(snapshot 単位)→ manifest(Avro、data/delete file 一覧 + partition tuple + metrics)。ディレクトリ列挙をメタデータ木で置換。**hidden partitioning**(transform で partition 値を導出)。v4 で相対パス化(再配置可) | snapshot ベース、schema evolution は列 ID | manifest レベルの file 統計 |
| **Delta Lake** | `_delta_log/` の**逐次トランザクションログ**(JSON、追記のみ、add/remove file 列挙) | **Copy-on-Write + deletion vector**(Parquet 内の行を削除マーク、即時 rewrite を回避) | per-file min/max のみ(record-level index 無し) |
| **Hudi** | timeline(1.0 で **LSM ツリー**化、long-term retention と planning 高速化) | **Merge-on-Read**(変更列のみ Avro delta log、write 増幅最小、読み取りで base+delta マージ)/ Copy-on-Write | **record-level index**(キー → file group の決定的ルックアップ、upsert 効率) |

出典: <https://iceberg.apache.org/spec/> /
<https://risingwave.com/blog/apache-iceberg-vs-delta-lake-vs-hudi-2026/> /
<https://hudi.apache.org/blog/2026/08/12/hudi-vs-delta-lake-for-write-heavy-workloads/>

- **Apache Paimon / Fluss(2026 の新顔)**: Paimon(旧 Flink Table Store、
  2024 TLP)は **lake format + LSM ツリー**を組み合わせ、**1 テーブルが
  message queue(streaming 購読)と batch テーブル(分析)を兼ねる**
  「streaming lakehouse」。primary-key テーブルは LSM level に整理され、
  writes は sorted run に着地して下位へ継続 compaction。**merge engine**
  (dedup / partial-update / aggregate)が「キーの最新状態」の意味を定義し、
  CDC セマンティクスをネイティブに持つ(delete を後付けの仕掛けにしない)。
  Fluss は sub-second の「hot」層で Paimon(「cold」層)と階層化。
  <https://paimon.apache.org/docs/1.3/> /
  <https://amdatalakehouse.substack.com/p/lakehouse-table-formats-in-2026-iceberg>
  → aruaru-db の「Git-on-SQL コミット = changelog、`AS OF COMMIT` =
  time travel」は Paimon の changelog + snapshot に発想が近い。**merge
  engine の partial-update / aggregate** は A.6-4 の Merge-on-Read に
  取り込む価値がある(将来。まず deletion vector)。
- → aruaru-db `table_format.rs` は **Iceberg 型のメタデータ木 + Databend 型の
  MetaSrv CAS** を採用済み。**取り込む価値がある未実装**:
  - **deletion vector**(Delta 型): object-table の block に「削除された行の
    ビットマップ」を持たせ、prune 時に適用 → 即時 rewrite 無しの DELETE。
    A.6-4 の delta 層の一形態として最有力。
  - **record-level index**(Hudi 型): キー → block の索引。現状は bloom
    filter による**否定的**枝刈りのみで、**肯定的**な直接ルックアップが無い。
  - **Merge-on-Read の base+delta マージ読み取り**(Hudi 型): A.6-4 の
    DeltaTree と本質同じ。

#### A.1.9 ベクトル化実行(Photon / DuckDB)と型認識圧縮

- **Photon**(Databricks、SIGMOD 2022 Best Industry Paper
  <https://people.eecs.berkeley.edu/~matei/papers/2022/sigmod_photon.pdf>):
  C++ の**ベクトル化**クエリエンジン。Delta Lake / Parquet を最小前提で
  処理。**deletion vector** で MERGE/UPDATE/DELETE を最大 10x。
- **DuckDB storage**(<https://duckdb.org/docs/current/internals/storage> /
  <https://duckdb.org/2022/10/28/lightweight-compression>):
  256KB 固定ブロック、行を **row group**(水平パーティション)へ、列は
  DSM(列指向)。**型認識軽量圧縮**を analyze フェーズで選択:
  constant / RLE / bitpacking / frame-of-reference / **dictionary** /
  **FSST**(最大 255 個の頻出バイト列を 1 バイトコードへ、文字列内の
  繰り返しも圧縮)/ **ALP**(浮動小数点、ベクトル化前提で高速 + 高圧縮
  <https://ir.cwi.nl/pub/33334/33334.pdf>)/ Chimp / Patas。
  目標は**スキャン時の高速展開**であって最大圧縮率ではない。
- → aruaru-db `olap.rs` は既に **Apache Arrow + DataFusion** の
  ベクトル化実行 + `arrow::compute::filter_record_batch` 等のカーネルを
  使用。**取り込む価値**: 辞書エンコードは適応選択済みだが、**RLE /
  bitpacking / FSST** は未実装(A.6-5)。ALP は数値列圧縮の候補。

---

### A.2 aruaru-db が既に取り込んでいるもの(2026-08-31 時点、crate 対応)

| 系統 | 取り込み済みの技術 | crate / モジュール |
|---|---|---|
| CockroachDB / TiKV | Multi-Raft(Range 単位の独立合意グループ、split/merge/scatter-gather) | `aruaru-dist/src/multi_raft.rs`, `shard/topology.rs` |
| CockroachDB / TiKV safe-ts / YugabyteDB | closed timestamp / follower read / bounded staleness / exact staleness、side transport(バイナリ化済み) | `aruaru-dist/src/closed_ts.rs`, `raft/binary_transport.rs` |
| CockroachDB Serverless / TiDB Serverless | ephemeral SQL pod(使い捨てプロセス、テナント別スナップショット) | `aruaru-server/src/ephemeral_pod.rs` |
| CockroachDB(キー空間プレフィックス方式のテナント分離) | `execute_as_tenant`(`__tenant_{id}__` プレフィックス) | `aruaru-query/src/engine.rs` |
| Neon | safekeeper quorum(term fencing + `flushLsn[n-quorum]`)+ pageserver(`get_page_at_lsn` + image layer GC cutoff)+ バックプレッシャ | `aruaru-dist/src/wal_service.rs` |
| Snowflake / Iceberg / Databend | 不変 snapshot→segment→block 3層メタデータ、min/max ゾーンマップ、bloom filter 等値枝刈り、MetaSrv 楽観的 CAS = コミット、`prev_snapshot_id` 連鎖の time travel | `aruaru-backup/src/table_format.rs` |
| ClickHouse MergeTree / DuckDB row group / SingleStore segment | セグメント単位ゾーンマップ(既定 1024 行、`RecordBatch::slice` ゼロコピー枝刈り)、行→列インクリメンタルマージ(TiFlash delta 発想の単一プロセス版)、`tokio::mpsc` による非同期購読 | `aruaru-query/src/olap.rs` |
| DuckDB / Parquet | 適応的辞書エンコード(ユニーク比率 < 0.7 のときのみ) | `aruaru-query/src/olap.rs` |
| ScyllaDB / Seastar | shard-per-core shared-nothing ストア、murmur3 token-aware routing | `aruaru-query/src/sharded_store.rs` |
| Neon(ブランチング)| 任意コミットからの CoW ブランチ(実データが切り替わる) | `aruaru-core/src/version/mod.rs`(`create_branch_from`)、`aruaru-query/src/engine.rs` |
| FoundationDB | 決定的シミュレーションテスト(seed 再現、フォールト注入、Log Matching 検証) | `aruaru-dist/src/raft/sim.rs` |
| Vitess Reshard / VTGate | Range 併合、scatter-gather 読み取り | `aruaru-dist/src/multi_raft.rs` |
| 独自 | Git-on-SQL(`aruaru_commit` / `AS OF COMMIT` / branch / merge)= protected timestamp + zero-copy clone + time travel を 1 機構で | `aruaru-core/src/version/`, `aruaru-query/src/engine.rs` |

---

### A.3 未取り込み技術と、取り込み判断(2026 再設計の中核)

#### A.6-1 HLC(Hybrid Logical Clock)による版付け — **実装済(2026-08-31)**

- 現状: `closed_ts.rs` は論理ナノ秒を**呼び出し側が渡す**前提。クロック
  スキュー上限(CockroachDB `max_offset`)の管理が無い。
- *実装方法*(論文「Logical Physical Clocks and Consistent Snapshots in
  Globally Distributed Databases」/ `cockroach/pkg/util/hlc/hlc.go`):
  timestamp = **物理成分 `pt`(≒ローカル wall time)+ 論理成分 `l`**。
  - `now()`: `pt' = max(pt, wall_now())`; `pt'==pt` なら `l++` else `l=0`; 返す。
  - `update(remote_pt, remote_l)`: `pt' = max(pt, remote_pt, wall_now())`;
    3 者の max のどれに一致したかで `l` を更新(単調増加を保証)。
  - **送信時**に `now()` を相乗り、**受信時**に `update()`。NTP のズレを
    吸収しつつ単調 + 因果順序。
- 判断: **取り込む**。CockroachDB / YugabyteDB / Spanner が全て HLC で
  「因果順序 + 実時刻近似」を得ている。`aruaru-dist` に `hlc.rs`
  (`Hlc { pt: u64, l: u32 }`、`now()` / `update()`)を新設し、`closed_ts`・
  `wal_service`・`multi_raft` の timestamp 源を HLC へ差し替える。
  ノード間メッセージ(`binary_transport.rs`)のフレームへ HLC を相乗り。
  <https://github.com/cockroachdb/cockroach/blob/master/pkg/util/hlc/hlc.go>
- スコープ注意: 真の分散クロック同期(NTP / TrueTime)は環境依存のため、
  「単一プロセス内の HLC + ノード間メッセージに HLC を相乗り」までとする
  (正直な簡略化点として明記)。`max_offset` による不確実性ウィンドウ
  (CockroachDB の `commit-wait`)は次段階。

#### A.6-2 Raft-Learner 上の 行→列 非同期変換レプリカ — **実装済(2026-08-31、実プロセス間で検証済み)**

- 現状: `--raft-role learner` で複製先にはなるが、learner が受け取った
  Raft ログを **行→列変換して独立の解析ストアへ流す**部分が無い。
  `olap.rs` の `OlapCache` は同一プロセス内の共有メモリ購読で、
  `aruaru-dist` の Raft 複製ログを経由していない。
- 判断: **取り込む**。これが「TiDB/TiFlash 型 HTAP」の核心であり、
  SET(RPoem + aruaru-db)の価値(「REST 不要・列解析も 1 グラフ」)を
  直接強化する。設計:
  1. learner ノードに `ColumnarApplier`(`Applier` trait の実装)を注入。
     Raft commit ごとに `WalRecord` 相当を受け取り、`table_format` の
     block へ列変換して追記(TiFlash の delta 空間)。
  2. 一定量たまったら `table_format` の segment へ compaction(stable 空間)。
  3. 読み取りは `closed_ts` の gate + **learner の apply 済み Raft index**
     を突合(A.6-3)して SI を保証。
  4. 「どのテーブルに列レプリカを何個」は **execution-config**
     (`ALTER TABLE ... SET TIFLASH REPLICA n` 相当)で宣言(§5・P5)。
- スコープ注意: ネットワーク越しの真の別ノード learner は
  `binary_transport.rs` の複製が前提(P4 以降)。まずは単一プロセス内で
  「learner 用 `ColumnarApplier` へ Raft ログを流す」配線を作る。

#### A.6-3 読み取り時の Raft index + MVCC による SI 検証 — **取り込む(A.6-2 と一体)**

- 現状: `closed_ts` の `can_serve_read_at` は「read_ts ≤ closed_ts」だけ。
  TiFlash はさらに「その read_ts に対応する **Raft log index** を learner が
  apply 済みか」を確認してから読む。
- *TiFlash の実アルゴリズム*(中国語一次資料 `tech.ipalfish.com` /
  `book.tidb.io` で確認): 読み取り要求を受けた learner は
  (1) leader へ **read-index** リクエストを送り、(2) その時点の
  commit index を取得、(3) ローカルの apply がその index に**追いつくまで
  待って** Raft ログを replay、(4) 要求 timestamp で MVCC フィルタして返す。
  <https://tech.ipalfish.com/blog/2020/09/08/tidb_htap/> /
  <https://book.tidb.io/session1/chapter9/tiflash-architecture.html>
- 判断: **取り込む**。`ColumnarApplier` に `applied_raft_index()` を持たせ、
  `plan_follower_read` の判定へ「leader の commit index を取得 →
  `commit_index ≤ applied_index` になるまで待つ(タイムアウトあり)→
  読み取り」の read-index ステップを追加。単一プロセスでは leader/learner が
  同居するため read-index は関数呼び出しで済む(P4 のネットワーク越し
  learner で初めて実 RPC 化)。

#### A.6-4 DeltaTree / deletion vector / MoR — **取り込む(段階的)**

- 現状: `table_format` の block は不変。行の**更新・削除**を表現できず、
  DELETE 相当は「新 snapshot で block を差し替え」= 実質 Copy-on-Write の
  重い経路のみ。
- 判断: **取り込む**。優先度順:
  1. **deletion vector**(Delta / Photon 型): `BlockMeta` に「削除行の
     RoaringBitmap 相当」を持たせ、`prune`/読み取りで適用。即時 rewrite 無しの
     DELETE/UPDATE。実装が最も軽く効果が大きい。
     *実装方法(Delta PROTOCOL.md より)*: DV は「data file 内の行位置の集合」を
     RoaringBitmap 配列で表す。64bit 位置を「上位 32bit = key / 下位 32bit =
     sub-position」に分け、key ごとに 32bit Roaring bitmap を 1 つ持つ
     (行数が 2^32 を超えるケースへの対応)。DV ファイルはテーブルルートに
     data file と並べて置き、1 ファイルに複数 data file 分の DV をシリアライズ。
     <https://github.com/delta-io/delta/blob/master/PROTOCOL.md> /
     <https://delta.io/blog/2023-07-05-deletion-vectors/>
  2. **base + delta の Merge-on-Read**(Hudi / TiFlash 型): segment に
     delta log(追記のみ)を併設し、読み取りで base+delta をマージ。
     A.6-2 の learner 列変換と同じ機構を流用できる。
     *実装方法(TiFlash DeltaTree より)*: 新規 insert/delete/update をまず
     **delta 空間**へ TiKV 生成順のまま追記(WAL 的、linearizability 維持)。
     一定量たまったら小ファイルへ compaction し、最終的に **stable 空間**
     (不変・列指向 chunk、Parquet 類似 + LZ4)へマージ。読み取りは
     stable(base)+ delta を on-the-fly でマージ。
     <https://github.com/pingcap/tiflash/blob/master/docs/design/0000-00-00-architecture-of-distributed-storage-and-transaction.md>
  3. **record-level index**(Hudi 型): キー → block の**肯定的**索引。
     現状の bloom(否定的枝刈り)を補完。
- スコープ注意: RoaringBitmap の外部 crate 依存は避け、`Vec<u64>` の
  ソート済み集合 or 単純ビットベクタで最小実装(既存方針)。Delta の
  「key ごとに 32bit bitmap」レイアウトは、2^32 行を超えない前提なら
  単一ビットベクタで足りる(超えたら Delta と同じ分割へ拡張)。

#### A.6-5 型認識軽量圧縮(RLE / bitpacking / FSST / ALP) — **保留(条件付き取り込み)**

- 現状: 適応的辞書エンコードのみ。DataFusion/Arrow がスキャンの
  ベクトル化は担うが、**格納サイズ**の圧縮は辞書止まり。
- 判断: **保留**。理由はコストではなく前提——現状の想定データ規模
  (単一プロセス内メモリ常駐)では DataFusion のストリーミング + 辞書で
  足りる。**実データ規模がボトルネックになった時点で**、DuckDB の
  analyze フェーズ(セグメントごとに複数方式を試算し最小を選ぶ)を
  簡易移植し、RLE → bitpacking → FSST の順で足す。ALP は数値列専用。
- *参考モデル*(Photon SIGMOD 2022): Photon は**実行時に column batch の
  メタデータ(NULL-ness / activeness)を組み立て、それを使ってカーネルを
  選ぶ**「adaptive execution」。aruaru-db の `olap.rs` の適応的辞書
  エンコード(ユニーク比率で分岐)は既にこの発想の小規模版。将来の
  analyze フェーズも「セグメントの実データを見てから方式を選ぶ」という
  Photon / DuckDB 共通の設計に沿わせる。
  <https://people.eecs.berkeley.edu/~matei/papers/2022/sigmod_photon.pdf>

#### A.6-6 SingleStore「1 テーブル内で行 segment と列 segment 同居」 — **A.6-4 に吸収**

- Universal Storage の hash index / subsegment access / row-level lock は、
  「不変列 segment に対して行単位の更新を可能にする」ための仕掛け。
  aruaru-db では A.6-4 の deletion vector + delta log が同じ目的を果たす
  ため、SingleStore 固有機構の個別移植はしない(重複)。

#### A.6-7 ClickHouse SharedMergeTree の Keeper — **取り込み済みの再確認**

- 「共有メタデータの真実源 = 外部(Keeper)、計算ノードはステートレスで
  メタデータを subscribe」というモデルは、本文書 §2 原則 2
  (コントロールプレーン = RPoem)・`table_format` の MetaService(CAS)・
  P5 の execution-config ポーリングと**既に同型**。新規取り込みは不要だが、
  RPoem の schema-registry を「Keeper 相当の共有メタデータストア」として
  位置づける記述を P5 に足す(A.7)。

#### A.6-8 Aurora 型モノリシックストレージ — **取り込まない(明示)**

- 理由: Neon 型の safekeeper/pageserver 分離の方が、follower read /
  ephemeral pod / 列レプリカ(A.6-2)と組み合わせやすく、`wal_service.rs`
  で既に分離型を実装済み。モノリシック化は後退。

---

### A.7 本再設計(管理面)への具体的な反映

1. **§5 `aruaru.yaml` スキーマに `htap` セクションを追加**(P3〜P5 で実装):
   ```yaml
   htap:
     columnar_replicas:            # A.6-2: TiFlash REPLICA n 相当
       - table: "orders"
         replicas: 1
         node_selector: "role=learner"
     read_consistency: "raft_index_checked"   # A.6-3: si | closed_ts_only | raft_index_checked
     delta:                        # A.6-4
       deletion_vectors: true
       merge_on_read: false
   ```
   これは「望ましい状態の宣言」(§2 原則 12)であり `setX` mutation は作らない。
2. **execution-config(RPoem 配信)**で `columnar_replicas` をクラスタ全体へ
   配布(ClickHouse SharedMergeTree の Keeper、TiDB の PD 相当)。
   `open-runo-schema-registry` を「共有 HTAP メタデータストア」と位置づける。
3. **observability**: `Query.closedTimestamp` に `applied_raft_index` を追加、
   `Query.htapReplicas`(列レプリカの遅延・行数)を新設(GraphQL、B3)。
4. **P4 のバイナリトランスポート**は A.6-2 のネットワーク越し learner 複製の
   前提。closed timestamp side transport は既にバイナリ化済み(`binary_transport.rs`)。

---

### A.8 2026 年の業界の現実(誇張しないための注記)

- 2026 時点でも、**単一エンジン HTAP DB は「専用 OLTP + 専用列指向 OLAP を
  CDC で繋ぐ」構成に対して支配的なシェアを取れていない**。標準的な本番
  パターンは依然として「2 つの専用 DB を CDC(Change Data Capture)で連結」
  (統合レイテンシは時間 → 秒へ短縮)。
  <https://bigdataboutique.com/blog/oltp-vs-olap-2026> /
  <https://clickhouse.com/resources/engineering/unifying-oltp-and-olap>
- 行 vs 列・point vs scan の I/O パターンの根本的トレードオフは消えていない。
  TiDB(TiFlash)/ SingleStore が「1 システム HTAP」の代表だが、
  いずれも内部では**行ストアと列ストアをレプリカとして分離**している。
- **aruaru-db の立ち位置(2026 再確認)**: 「1 プロセスに全部入れる」こと
  自体が目的ではない。核は (a) Git-on-SQL による厳密な版管理・time travel、
  (b) Raft 強整合 OLTP、(c) それと**非同期**に繋がる列指向 OLAP レプリカ
  (A.6-2)、(d) PostgreSQL との DUAL DATABASE = CDC 相当(実装済み)。
  つまり業界標準の「専用 OLTP + 専用 OLAP + CDC」を、**単一 Pure Rust
  バイナリで運用でき、かつコミット単位の版管理が付く**形にまとめたもの。
  「単一エンジン HTAP が万能」という過大な主張はしない。

---

## 付録 B. 「REST 完全撤廃」を可能にしている WunderGraph Cosmo の技術（2026-08-29 再調査）

ユーザー指摘: 「REST API を撤廃したのは WunderGraph Cosmo の技術あってこそ。
分かっているか」。英日・多言語で Google / GitHub を再調査した結論を、
**どの機構が何を代替するか**の形で整理する。RPoem はこれらを OSS で
自前実装する（Cosmo 本体 Apache 2.0、有料は SSO+SCIM+専有クラウドのみ）。

### B.1 4つの中核機構

| Cosmo の機構 | 何をするか | これが無い場合に必要になる REST | RPoem 側の受け皿 |
|---|---|---|---|
| **GraphQL Federation / Open Federation**（Apollo v1/v2 互換。`@key` / `_entities` によるエンティティ解決、`wgc router compose` が execution config を生成、Router がクエリプランニング・バッチング・フィールド解決）| 多数のサービスを**1 つの GraphQL エンドポイント**に合成。クライアントは supergraph だけを見る | サービスごとの REST エンドポイント群 | `open-runo-federation`（合成）、`aruaru-server` は既にネイティブ async-graphql サブグラフ |
| **Cosmo Connect / Protocol-Agnostic Federation**（`wgc router plugin generate` が GraphQL SDL → protobuf。**router plugin バイナリ**〈Router がプロセス内管理〉または **standalone gRPC service** として実行。既存の REST/SOAP/SDK バックエンドは RPC 実装の**内側にラップ**。Router が gRPC を直接呼ぶ）| GraphQL サーバを書かずに**非 GraphQL バックエンドをサブグラフ化**。「作り直さず、今あるものを繋ぐ」 | レガシー REST をそのまま外部公開し続ける | RPoem の `open-runo-router` レガシー REST ハンドラ（`handlers_hyper.rs` 等）は、GraphQL 化が重いものは Connect 型（gRPC サブグラフ / プラグイン）で federate。`open-runo-poem-compat` の考え方と整合 |
| **Persisted Operations / Trusted Documents**（`wgc operations push` → 制御プレーンが SHA-256 を採番 → **CDN 配信**。enforcement: 未登録を log / safelist 一致のみ allow / 全ブロック。`graphql-client-name` ヘッダでクライアント別オペレーション集合）| 旧 REST エンドポイント 1 本 = **名前付き・許可リスト済みオペレーション 1 個**。クライアントはハッシュだけ送る。帯域減 + 攻撃面減 | 「固定エンドポイント」というセキュリティ/キャッシュ境界 | **`open-runo-persisted-queries`（実装済み）** — `sha256Hash` / `query`、`extensions.persistedQuery` 対応。Cosmo が有料に制限する trusted documents を OSS で |
| **Schema Registry + Composition Checks + CDN 配信**（`wgc` が制御プレーンへ、Router は **完全ステートレス**で高可用 CDN から execution config を取得。§1 参照）| 宣言的な設定/スキーマの配布パイプライン | 設定を叩き込む管理 REST | `open-runo-schema-registry`（= Cosmo の Control Plane + CDN 相当）、`aruaru-server` は取得側（§2 原則 2） |

### B.2 補助機構

- **EDFS / Cosmo Streams**（NATS / Kafka / Redis を「仮想サブグラフ」として
  Router が直結。サブグラフ側にステートフル接続〈WebSocket〉不要、
  epoll/kqueue で数万サブスクリプション、同一トピックのサブスクリプションを
  Router 内で重複排除）→ **Webhook / SSE / ロングポーリングの REST パターンを
  置換**。aruaru-db の変更フィード（closed timestamp の伝播、object-table の
  コミット通知など）はこの形へ。ノード間 side transport のバイナリ化（§3 P4）
  とは層が別（こちらはクライアント向けリアルタイム）。
- **Router plugins（Go）**: Router 自体を自前コードで拡張。Router は
  ステートレスで、依存は「高稼働率 CDN から config を読む」ことだけ
  → §2 原則 1「データプレーンはコントロールプレーンが落ちても動く」の根拠。

### B.3 だから aruaru-db + RPoem では

1. **aruaru-db** はネイティブ async-graphql サブグラフのまま
   （`/admin/*` → B2/B3 リゾルバ + B1 は `aruaru.yaml`）。
2. **RPoem のレガシー REST** は、GraphQL 化が軽いものは素直に移植、
   重いものは **Cosmo Connect 型（gRPC サブグラフ / router plugin）** で
   federate（書き直さず繋ぐ）。
3. **旧 REST エンドポイント 1 本ごとに Persisted Operation を 1 個**採番
   （`open-runo-persisted-queries`）。クライアントはハッシュ送信。
   enforcement は safelist。→ 「固定エンドポイント」が持っていた
   キャッシュ境界・攻撃面の縮小・契約の安定性を GraphQL 上で回復する。
4. **execution config / persisted operations / feature flags の配信**は
   `open-runo-schema-registry` が CDN 役（Router = `aruaru-server` /
   `open-runo-router` は取得側・ステートレス）。
5. **リアルタイム**は EDFS 型（イベントソース直結）へ。ポーリング REST を残さない。

**要するに「REST を消せる」のは、Federation がサービスを 1 グラフに畳み、
Connect が非 GraphQL バックエンドを書き直さず federate でき、Persisted
Operations が固定エンドポイントの利点を GraphQL 上で再現し、Schema
Registry + CDN が設定を宣言的に配れるから。** この 4 つが揃って初めて
「REST 完全撤廃」が実務的に成立する。
