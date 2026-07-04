# DATABASE.md — aruaru-DB データベース設計仕様
# 2026-06-21 最新版

---

## §1 概要

aruaru-DB は「仕様変更に強い」分散データベースを目標とする。  
変更に強い = **スキーマの変更履歴を追跡できる** × **データのバージョンを Git のように管理できる**。

---

## §2 ストレージ層

### §2a Row Store (OLTP)
- エンジン: **fjall** (Pure Rust LSM ツリー)
- MVCC キー: `[table_id:4B][pk...][ts:8B desc]`
- タイムスタンプは単調増加 u64 (HLC: Hybrid Logical Clock)
- 書き込みは WAL → memtable → L0 SST の順

### §2b Columnar Store (OLAP)
- フォーマット: **Apache Parquet** (Zstd 圧縮デフォルト)
- バッファ: **Apache Arrow** RecordBatch (ベクトル化)
- Row Store から自動変換 or Parquet ファイルを直接読み込み
- DataFusion の TableProvider に接続

### §2c Version Tree (Git-on-SQL)
- 構造: **Prolly Tree** (コンテンツアドレッサブル B-tree)
- チャンクサイズ: 平均 4KB (チャンク境界はローリングハッシュで決定)
- diff: O(変更量) — 同一ハッシュのチャンクはスキップ
- ルートハッシュ = コミット ID の材料

---

## §3 クエリ層

### §3a HTAP ルーター
```
クエリ受信
  ↓
クエリ分類
  ├─ OLTP (点検索・短トランザクション) → fjall 行ストア
  └─ OLAP (集計・JOIN・GROUP BY)        → DataFusion 列ストア
```

### §3b DataFusion 統合 (Snowflake 的 OLAP)
- Apache DataFusion を計算エンジンとして採用
- ストレージ/コンピュート 分離: DataFusion Worker は独立スケール
- 仮想ウェアハウス: SessionContext を WH 単位で独立化
- プッシュダウン: predicate / projection を Parquet 読み込み時に適用

---

## §4 バージョン管理 (Git-on-SQL)

### §4a App レベル Git-on-SQL

**コミットモデル**
```
Commit {
  id:             SHA-256(parent || root_hash || author || message || ts)
  parent:         Option<CommitId>
  root_hash:      [u8; 32]   ← Prolly Tree 全データの指紋
  author:         String
  message:        String
  timestamp:      i64 (Unix nanoseconds)
  schema_version: u32
}
```

**SQL インターフェイス**
```sql
-- ブランチ作成
SELECT aruaru_branch('feature/new-schema');

-- 現在のブランチで作業
ALTER TABLE products ADD COLUMN tags TEXT[];

-- コミット
SELECT aruaru_commit('Add tags column to products');

-- ログ
SELECT * FROM aruaru_log ORDER BY timestamp DESC LIMIT 20;

-- diff (2コミット間)
SELECT * FROM aruaru_diff('main', 'feature/new-schema')
WHERE table_name = 'products';

-- マージ
SELECT aruaru_merge('feature/new-schema', 'main');

-- タイムトラベル (過去のスナップショットを SELECT)
SELECT * FROM products AS OF COMMIT 'abc123def456';
SELECT * FROM products AS OF TIMESTAMP '2026-06-01 00:00:00';
```

**GraphQL インターフェイス**
```graphql
mutation CreateBranch {
  createBranch(name: "feature/new-schema") { success message }
}

mutation CommitChanges {
  commit(author: "PHI", message: "Add tags column") {
    success commitId message
  }
}

query GetLog {
  log(limit: 20) {
    id shortId author message timestamp
  }
}

query GetDiff {
  diff(from: "main", to: "feature/new-schema") {
    added removed modified
    rows { tableName pkHex kind }
  }
}
```

---

### §4b DoltgreSQL 互換性

DoltgreSQL (pre-alpha, Go 製) との互換性は **プロトコルレベル** で維持する方針。

- ワイヤプロトコル: PostgreSQL (psql 互換) → 共通
- SQL 関数名: `dolt_commit()` → `aruaru_commit()` のエイリアス提供
- システムテーブル: `dolt_log` → `aruaru_log` (同構造)
- 移行: DoltgreSQL → aruaru-DB は `pg_dump` 経由でゼロリスク

**DoltgreSQL を採用しない理由**
- Go 製 → 性能の上限が低い (Rust との比較)
- pre-alpha → 本番使用不可
- エミュレーション層 → 本物の PostgreSQL バイナリではない

---

### §4c CockroachDB 互換性

CockroachDB v24.3 以降は非 OSS (CockroachDB Software License)。  
aruaru-DB は**設計思想**を採用し、**コードは採用しない**。

採用する設計思想:
- Range ベース自動シャーディング
- Raft 合意 (openraft で実装)
- MVCC によるタイムスタンプ並行制御
- PostgreSQL ワイヤ互換 (pgwire で実装)

移行ツール (`aruaru-migrate`):
```bash
# CockroachDB → aruaru-DB
aruaru-migrate \
  --source cockroach \
  --source-uri "postgres://root@cockroach:26257/mydb" \
  --commit-message "Migrate from CockroachDB"
```

---

## §5 分散アーキテクチャ (openraft)

```
Node-1 (Leader)  ←→  Node-2 (Follower)  ←→  Node-3 (Follower)
   ↕ Raft Log
Range-001: pk [   ... 'M')   # 64MB
Range-002: pk ['M' ... 'Z')  # 64MB
Range-003: pk ['Z' ... ]     # 64MB
```

- 各 Range が独立した Raft グループ
- Range split: 128MB 超で自動二分割
- Meta Range: ノード↔Range マッピングを管理

---

## §6 仮想ウェアハウス (Snowflake 互換概念)

```sql
-- 分析用大型 WH
CREATE WAREHOUSE analytics_wh
  WITH SIZE = 'X-LARGE'
  AUTO_SUSPEND = 300
  AUTO_RESUME = TRUE;

-- アプリ用小型 WH
CREATE WAREHOUSE app_wh
  WITH SIZE = 'SMALL'
  AUTO_SUSPEND = 60;

-- WH を選択してクエリ
USE WAREHOUSE analytics_wh;
SELECT region, SUM(revenue) FROM sales
GROUP BY region
ORDER BY 2 DESC;
```

実装: DataFusion `SessionContext` を WH ごとに独立インスタンス化。  
CPU スレッド数 = SIZE に対応する定数で制御。

---

## §7 移行ツール仕様

| 移行元 | プロトコル | 推定速度 |
|--------|-----------|---------|
| PostgreSQL | pg_dump COPY | ~500K 行/秒 |
| CockroachDB | PostgreSQL 互換 | ~400K 行/秒 |
| Snowflake | Parquet COPY INTO | ~1M 行/秒 |
| MySQL | mysqldump + 変換 | ~300K 行/秒 |
| CSV | Arrow CSV reader | ~2M 行/秒 |
| Parquet | DataFusion 直読み | ~5M 行/秒 |

---

## §8 ロードマップ

| バージョン | 主要機能 | 目標時期 |
|-----------|---------|---------|
| v0.2 (現在) | スキャフォールド完成: ワークスペース・API面・Admin GUI・各言語ドライバ・CI | 2026 Q3 ✅ |
| v0.3 ✅ | Git-on-SQL(Prolly) + pgwire + 管理API + DataFusion OLAP(型付きHTAP) + 基本CRUD(INSERT/SELECT/UPDATE/DELETE/DROP) + fjall write-through永続化 + 対応DB150+/毎日クロール + PG/MySQL/Mongo/CQLアダプタ | 2026 Q4 |
| v0.4 ✅ | WunderGraph Cosmo サブグラフ化(Federation v2・実エンジン接続・SDL出力) + トランザクション(BEGIN/COMMIT/ROLLBACK・原子性/durability) + pgwire Extended Query(プリペアドステートメント) | 2027 Q1 |
| v0.5 ✅ | Raftコア(複製ログ・選挙・AppendEntries/RequestVote RPC・HTTPトランスポート・合意driver) + シャード配置/ルーティング + 書き込みRaftパスD + 管理アプリ(共通core/Windows .msi+exe/Web SPA) | 2027 Q2 |
| v0.6 | 仮想ウェアハウス + 移行ツール全種 + Snowflake Parquet import | 2027 Q3 |
| v0.7 | バックアップ/リストア完成 + Tauri Admin GA | 2027 Q4 |
| v1.0 | 本番準備完了・Jepsen テスト通過 | 2028 |

---

## §9 ライセンスと OSS 戦略

- **Apache License 2.0** — 商用利用・改変・再配布すべて自由
- CockroachDB のように source-available に移行しない (コミュニティへの約束)
- GitHub で公開、世界中のボランティアによってメンテナンス
- 採用クレートはすべて MIT / Apache-2.0 / BSD 系

---

*このドキュメントは aruaru-DB プロジェクトの設計仕様書です。*  
*最終更新: 2026-06-21 by PHI / aruaru-DB Contributors*
