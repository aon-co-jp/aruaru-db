# aruaru-DB Architecture (2026-06-21 rev.)

## 1. 設計哲学

### 1-1. なぜ Pure Rust か

Go 製のエンジン（DoltgreSQL 等）は AST 変換を挟むエミュレーション層が性能の天井を決める。  
Rust は型・メモリ・スレッド安全をゼロコスト抽象で担保し、性能限界が OS レイヤに近い。  
`pgwire` / `datafusion` / `openraft` という三本柱がすべて Rust ネイティブで揃った 2026 年に  
Pure Rust でフルスタック分散 DB を作ることが現実解となった。

### 1-2. CockroachDB + Snowflake の良さどりと差別化

**CockroachDB から取り入れるもの（設計思想）**
- Raft による分散強整合 → `openraft` で実装
- MVCC によるタイムスタンプベース並行制御
- Range ベース自動シャーディング
- PostgreSQL ワイヤ互換 → `pgwire` で実装

**Snowflake から取り入れるもの（設計思想）**
- ストレージ / コンピュート 完全分離
- 列指向 + ベクトル化実行 → `datafusion` + `arrow`
- Parquet による圧縮・プッシュダウン
- 仮想ウェアハウス（コンピュートプール）スケール

**aruaru-DB 固有の独自性**
- **Git-on-SQL**: データとスキーマを Prolly Tree でバージョン管理
- **Versionless GraphQL**: エンドポイント一つでスキーマを進化させる
- **Tauri 管理 GUI**: ブランチ DAG / コミットログの視覚化
- **移行ウィザード**: Postgres / CockroachDB / Snowflake / CSV からの引越しツール内蔵
- **完全 Apache-2.0**: CockroachDB が 2024 年に商用専用化した空白を埋める

---

## 2. レイヤー詳細

### Layer 1: Storage

```
┌────────────────────────────────────────┐
│  Row Store         Columnar Store      │
│  ┌──────────┐     ┌──────────────┐    │
│  │  fjall   │     │ Arrow/Parquet│    │
│  │ (LSM)    │     │ (列指向)     │    │
│  └────┬─────┘     └──────┬───────┘    │
│       │                  │            │
│  ┌────┴──────────────────┴────────┐   │
│  │   Version Tree (Prolly Tree)   │   │
│  │   - Merkle ハッシュでコミット   │   │
│  │   - branch / tag ポインタ      │   │
│  └────────────────────────────────┘   │
│  ┌────────────────────────────────┐   │
│  │   WAL (Write-Ahead Log)        │   │
│  └────────────────────────────────┘   │
└────────────────────────────────────────┘
```

**Row Store (`fjall`)**
- Rust ネイティブ LSM ツリー実装
- OLTP ワークロード（点検索・小トランザクション）に最適化
- MVCC タイムスタンプを key prefix に付与: `[ts_u64_be][pk...]`

**Columnar Store (Arrow / Parquet)**
- 分析クエリは Row Store から自動変換 or Parquet ファイルを直接 mmap
- SnowflakeのPAX形式に近い列グループ配置
- DataFusion の TableProvider インターフェイスで接続

**Version Tree (Prolly Tree)**
- Dolt が先行実装した概念を Pure Rust で再実装
- チャンク境界を内容ハッシュで決定 → diff が O(変更量) で済む
- `git commit`と等価: 全行データのルートハッシュ = コミット ID

### Layer 2: Query & Distribution

```
SQL テキスト
    ↓
sqlparser-rs (AST)
    ↓
HTAP Router ─── OLTP 判定 ──→ 行ストア executor
    │
    └────────── OLAP 判定 ──→ DataFusion (列指向・ベクトル化)
                                    ↓
                               openraft (分散合意)
                                    ↓
                           Range Shard Manager
```

**HTAP Router の判定基準**
- `SELECT ... LIMIT n` かつ PK / インデックス使用 → OLTP path
- `GROUP BY / WINDOW / JOIN 複数テーブル / 集計` → OLAP path
- 明示的: `/*+ OLAP */` ヒント

**openraft 統合**
- 各 Range（デフォルト 64MB）が独立した Raft グループを持つ
- Leader が書き込みを受付、Follower がレプリカ
- ログエントリ = WAL レコードと 1:1

**Range Sharding**
- CockroachDB と同じ key-range ベース
- 自動スプリット: Range が 128MB を超えたら二分割
- メタ Range がノードマップを管理

### Layer 3: Access

**pgwire (PostgreSQL ワイヤ互換)**
- 既存の psql / DBeaver / Tableau がそのまま繋がる
- Simple Query + Extended Query (プリペアドステートメント) 実装
- 認証: MD5 / SCRAM-SHA-256

**GraphQL (Poem + async-graphql)**
- Versionless: フィールド追加は non-breaking、削除は `@deprecated` → 安全な廃止猶予
- 型: `Query`（読み取り）/ `Mutation`（書き込み・コミット・ブランチ）/ `Subscription`（変更ストリーム）
- エンドポイント: `POST /graphql`、GraphiQL: `GET /graphql`

**Tauri 2 Admin GUI**
- React + Vite フロントエンド
- Rust コマンド経由で aruaru-server と通信
- 機能: ブランチ DAG ビジュアライザ / コミットログ / クエリエディタ / 移行ウィザード

---

## 3. Git-on-SQL の実装詳細

### コミット構造体

```rust
pub struct Commit {
    pub id: [u8; 32],          // SHA-256 of (parent + root_hash + meta)
    pub parent: Option<[u8; 32]>,
    pub root_hash: [u8; 32],   // Prolly Tree root
    pub author: String,
    pub message: String,
    pub timestamp: i64,        // Unix nanoseconds
    pub schema_version: u32,
}
```

### ブランチ

```
main    ──→ commit_A ──→ commit_B ──→ commit_C
                                          ↑ HEAD
feature ──→ commit_A ──→ commit_D ──→ commit_E
                                          ↑ HEAD
```

### Diff アルゴリズム

1. 左コミットの Prolly Tree ルートと右ルートを比較
2. チャンクハッシュが同じ = 変更なし（スキップ）
3. 異なるチャンクのみ再帰的に展開
4. 変更行 `(pk, before, after)` を Parquet ストリームで返却

### Merge 戦略

- **Fast-forward**: feature が main の子孫 → ポインタ移動のみ
- **3-way merge**: 共通祖先を起点に両ブランチの diff を適用
- **コンフリクト**: 同一 PK への異なる変更 → `aruaru_conflicts` テーブルに格納

---

## 4. 仮想ウェアハウス（Snowflake 互換概念）

```sql
-- コンピュートプールを定義（独立スケール）
CREATE WAREHOUSE analytics_wh WITH SIZE='LARGE' AUTO_SUSPEND=300;
CREATE WAREHOUSE app_wh WITH SIZE='SMALL' AUTO_SUSPEND=60;

-- クエリを特定 WH にルーティング
USE WAREHOUSE analytics_wh;
SELECT region, SUM(revenue) FROM sales GROUP BY region;
```

実装: DataFusion の SessionContext を WH ごとに独立インスタンス化し、  
スレッドプール（Tokio の `Runtime`）のサイズで「SIZE」を表現。

---

## 5. 移行ツール (aruaru-migrate)

| ソース | 方式 | 備考 |
|--------|------|------|
| PostgreSQL | `pg_dump` COPY + pgwire re-ingest | pgvector 対応 |
| CockroachDB | PostgreSQL 互換エクスポート経由 | BSL 条項に注意 |
| Snowflake | `COPY INTO <stage>` → Parquet → aruaru | ネイティブ Parquet 読み込み |
| MySQL / MariaDB | `mysqldump` → sqlparser 変換 | 型マッピング自動 |
| CSV / NDJSON | ストリーミング ingest | Arrow CSV reader |

---

## 6. VersionlessAPI 戦略

### GraphQL スキーマ進化ルール

```graphql
# ✅ 非破壊的変更: フィールド追加
type User {
  id: ID!
  name: String!
  email: String!
  score: Int          # v2 追加 - 既存クライアントに影響なし
}

# ✅ 廃止猶予: @deprecated
type User {
  username: String @deprecated(reason: "Use `name` instead. Remove in 2027-01.")
  name: String!
}

# ❌ 破壊的変更 (禁止): フィールド削除・型変更
```

### DB コミットとの連携

GraphQL Mutation が DB を変更する際、オプションで自動コミットを作成:

```graphql
mutation {
  insertUsers(data: [...], commit: { message: "Bulk import Q3" }) {
    commitId
    rowsAffected
  }
}
```

---

## 7. ロードマップ

| フェーズ | 目標 | 完了目安 |
|---------|------|---------|
| **v0.2** (現在) | スキャフォールド完成: ワークスペース構成・API面定義・Admin GUI・各言語ドライバ・CI/クロスビルド | 2026 Q3 ✅ |
| **v0.3** | Row Store + WAL + pgwire 基本 CRUD + Git-on-SQL (branch/commit/diff) | 2026 Q4 |
| **v0.4** ✅ | Cosmo サブグラフ化 + トランザクション + Extended Query | 2027 Q1 |
| **v0.5** | openraft 分散化 (3ノードクラスタ) | 2027 Q2 |
| **v0.6** | 仮想ウェアハウス + バックアップ/移行ツール完成 + Tauri Admin GA | 2027 Q3 |
| **v1.0** | 本番準備完了 + Jepsen テスト通過 | 2028 |

---

## 8. 採用 Rust クレート一覧

```toml
# ストレージ
fjall          = "2"       # Rust ネイティブ LSM
redb           = "2"       # 組込み key-value (メタデータ用)
arrow          = "55"      # Apache Arrow 列フォーマット
parquet        = "55"      # Parquet 読み書き

# クエリエンジン
datafusion     = "47"      # Apache DataFusion (Snowflake 的 OLAP)
sqlparser      = "0.52"    # SQL パーサ

# 分散
openraft       = "0.9"     # Raft 合意アルゴリズム

# プロトコル
pgwire         = "0.27"    # PostgreSQL ワイヤプロトコル
poem           = "3"       # HTTP サーバ
async-graphql  = "7"       # GraphQL エンジン

# ユーティリティ
tokio          = "1"       # 非同期ランタイム
serde          = "1"       # シリアライズ
sha2           = "0.10"    # ハッシュ (コミット ID)
uuid           = "1"       # UUID v7 (タイムスタンプ順)
thiserror      = "1"       # エラー型
anyhow         = "1"       # エラーハンドリング
tracing        = "0.1"     # 構造化ログ
```
