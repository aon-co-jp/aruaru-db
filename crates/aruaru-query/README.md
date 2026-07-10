# aruaru-query

aruaru-DB のクエリ実行エンジン。HTAP (OLTP + OLAP) を1クレートで担う。
Git-on-SQL (ブランチ/コミット/diff) は `aruaru-core::version` と接続済みで、
実データのコミット・巻き戻しが行える。

## 構成
- `engine`  — OLTP 実行エンジン本体。テーブルデータをインメモリ (`BTreeMap`) に保持し、
  `aruaru_commit` 時に全テーブルの行を Prolly Tree へスナップショットして
  `root_hash` を確定、`VersionController` にコミットとして記録する。
  BEGIN/COMMIT/ROLLBACK によるトランザクション、`fjall` 永続ストアへの
  write-through 永続化、冪等性キー付き実行 (`execute_idempotent`) も実装する。
- `parser`  — 軽量 SQL サブセットパーサ。完全な SQL パーサ (sqlparser) では
  なくコンパイル安定性・正しさを優先した手書きパーサで、pgwire を駆動するのに
  必要な文だけを解釈する。
- `olap`    — OLAP 実行経路 (Apache DataFusion)。HTAP ルーターが集計/GROUP BY/JOIN
  等と判定したクエリを DataFusion の列指向・ベクトル化・マルチパーティション
  実行エンジン (`target_partitions` 数だけ並列) に委譲する。単一ノード内 MPP
  であり、これがノードをまたぐ分散実行 (Ballista 型) の土台になる。

## 対応 SQL (v0.5 時点)
- `CREATE TABLE [IF NOT EXISTS] t (col1, col2, ...)`
- `INSERT INTO t (cols) VALUES (...)`
- `SELECT * FROM t` / `SELECT ... FROM t WHERE col = '...'`
- `UPDATE t SET col = '...' [WHERE ...]` / `DELETE FROM t [WHERE ...]` / `DROP TABLE t`
- `BEGIN` / `COMMIT` / `ROLLBACK` (単一・直列化トランザクション)
- Git-on-SQL 関数: `aruaru_branch` / `aruaru_checkout` / `aruaru_commit` / `aruaru_merge`
- システムテーブル: `aruaru_log`

## HTAP ルーティング
`classify_query(sql)` が `GROUP BY` / `SUM()` / `COUNT()` / `AVG()` / `WINDOW` を
含むクエリを `QueryKind::Olap` と判定する。`QueryEngine::execute_async` は
OLAP と判定されたクエリをまず DataFusion 経路 (`olap::run_olap`) で実行し、
失敗時のみ組み込み OLTP エンジンへフォールバックする。

## 現段階の制約
- ストレージは行=テキスト (全列 TEXT) が前提。数値集計は SQL 側で
  `CAST(col AS BIGINT)` のように明示キャストする (catalog の `ColumnType` →
  Arrow `DataType` の自動マッピングは次段階)。
- OLAP 実行は単一ノード内の並列 (MPP) まで。ノード間分散は `aruaru-dist`
  (openraft + Arrow Flight) 実装後に接続する。
- 完全な SQL パーサ (sqlparser) への置き換えは将来のマイルストーンで行う。

## 主な用途
- `aruaru-wire`: pgwire (PostgreSQL ワイヤプロトコル) サーバがクエリを
  `QueryEngine` に委譲する。
- `aruaru-graphql` / `aruaru-server`: 管理 GraphQL API (`AdminCtx.engine`) が
  レジストリ・クラスタ状態・マイグレーション結果の取得に利用する。
- `aruaru-backup`: `snapshot_tables()` で全テーブルを取得してバックアップし、
  `ingest_table()` でリストア時に書き戻す。
