# aruaru-registry

対応DBレジストリ（150+件）+ 毎日クロール + 取り込みアダプタ。

## 構成
- `types`     — DB分類 / ワイヤ / ステータス(5段階) / 移行経路 / バックアップ対応度
- `seed`      — 2026.06 時点の DB-Engines 上位＋著名DB 159件の初期データ
- `registry`  — レジストリ本体（検索・ステータス絞り込み・集計・クロール反映）
- `crawler`   — DB-Engines を主、フォールバック付きでランキング/スコアを取得
- `scheduler` — 24時間ごとの自動クロール（サーバ起動時に常駐）
- `adapter`   — capability(ワイヤ)単位の取り込みアダプタ

## ステータス5段階
| 段階 | 意味 |
|------|------|
| GA | ネイティブ完全対応 |
| Beta | 実装済み・検証中 |
| PgCompatible | PostgreSQLワイヤ互換として**実接続可能** |
| ReadOnly | 読み取り/取り込みのみ |
| Planned | レジストリ登録済み・未接続 |

## アダプタ戦略（capabilityベース）
150件すべてに個別ドライバを書くのではなく、**ワイヤプロトコル単位**で実装する。
`PgWireAdapter`（tokio-postgres、実接続）1本で、PostgreSQLワイヤ互換の
CockroachDB / YugabyteDB / Redshift / AlloyDB / Materialize / Citus / Greenplum /
RisingWave / QuestDB / CrateDB / Supabase / Neon / openGauss / GaussDB … を一括カバー。
✅ `MySqlAdapter`（mysql_async、実接続）も実装済み → MariaDB / TiDB / SingleStore / StarRocks / Apache Doris / Vitess / OceanBase / PolarDB / Percona / Aurora MySQL を一括カバー。✅ `MongoAdapter`(mongodb・実接続) → MongoDB / DocumentDB / Cosmos(Mongo API)、✅ `CqlAdapter`(scylla・実接続) → Cassandra / ScyllaDB もカバー。



## 毎日クロール
`scheduler::run_daily` がサーバ起動直後に1回 + 以後24時間ごとに
`DB-Engines Ranking` を取得し、名前正規化でレジストリに突き合わせて
rank / score / updated_at を更新する。取得失敗時はフォールバックソースへ。
