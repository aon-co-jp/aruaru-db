//! OLAP 実行経路 (DataFusion)
//!
//! HTAP ルーターが OLAP と判定したクエリ (集計・GROUP BY・JOIN 等) を
//! Apache DataFusion で実行する。DataFusion は列指向・ベクトル化・
//! マルチパーティションの実行エンジンで、`target_partitions` の数だけ
//! パーティション並列 (Volcano + RepartitionExec) で処理する。
//! = **単一ノード内 MPP**。これがノードをまたぐ分散実行 (Ballista 型) の土台になる。
//!
//! ## 現段階の制約
//! - ストレージは行=テキストのため、全列を Arrow の Utf8 として登録する。
//!   数値集計は SQL 側で `CAST(col AS BIGINT)` のように明示キャストする。
//!   (catalog の ColumnType → Arrow DataType 自動マッピングは次段階)
//! - 単一ノード並列まで。ノード間分散は分散レイヤ (openraft + Arrow Flight) 実装後。

use std::sync::Arc;

use datafusion::arrow::array::{
    ArrayRef, BooleanArray, Float64Array, Int64Array, StringArray,
};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::arrow::util::display::{ArrayFormatter, FormatOptions};
use datafusion::datasource::MemTable;
use datafusion::prelude::{SessionConfig, SessionContext};

use aruaru_core::catalog::ColumnType;

use crate::engine::{QueryEngine, QueryResponse, Value};

/// ColumnType → Arrow DataType
fn arrow_type(ty: &ColumnType) -> DataType {
    match ty {
        ColumnType::Int | ColumnType::BigInt => DataType::Int64,
        ColumnType::Float => DataType::Float64,
        ColumnType::Bool => DataType::Boolean,
        // Text / Bytes / Timestamp は当面 Utf8 (タイムスタンプ解析は次段階)
        _ => DataType::Utf8,
    }
}

/// 列の文字列値ベクタを Arrow 配列へ変換 (型に応じてパース)
fn build_array(ty: &ColumnType, cells: Vec<Option<String>>) -> ArrayRef {
    match ty {
        ColumnType::Int | ColumnType::BigInt => {
            let v: Vec<Option<i64>> = cells
                .into_iter()
                .map(|c| c.and_then(|s| s.trim().parse::<i64>().ok()))
                .collect();
            Arc::new(Int64Array::from(v)) as ArrayRef
        }
        ColumnType::Float => {
            let v: Vec<Option<f64>> = cells
                .into_iter()
                .map(|c| c.and_then(|s| s.trim().parse::<f64>().ok()))
                .collect();
            Arc::new(Float64Array::from(v)) as ArrayRef
        }
        ColumnType::Bool => {
            let v: Vec<Option<bool>> = cells
                .into_iter()
                .map(|c| {
                    c.and_then(|s| match s.trim().to_lowercase().as_str() {
                        "true" | "t" | "1" | "yes" => Some(true),
                        "false" | "f" | "0" | "no" => Some(false),
                        _ => None,
                    })
                })
                .collect();
            Arc::new(BooleanArray::from(v)) as ArrayRef
        }
        _ => Arc::new(StringArray::from(cells)) as ArrayRef,
    }
}

fn session_context() -> SessionContext {
    // 並列度 = 利用可能 CPU 数 (パーティション並列実行に使われる)
    let parallelism = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let config = SessionConfig::new().with_target_partitions(parallelism);
    SessionContext::new_with_config(config)
}

/// (列定義, 行データ) から Arrow の (Schema, RecordBatch) を構築する。
fn build_table_batch(
    columns: &[(String, ColumnType)],
    rows: &[Vec<String>],
) -> Result<(Arc<Schema>, RecordBatch), String> {
    let fields: Vec<Field> = columns
        .iter()
        .map(|(cname, cty)| Field::new(cname, arrow_type(cty), true))
        .collect();
    let schema = Arc::new(Schema::new(fields));

    let arrays: Vec<ArrayRef> = columns
        .iter()
        .enumerate()
        .map(|(ci, (_, cty))| {
            let cells: Vec<Option<String>> = rows
                .iter()
                .map(|r| r.get(ci).cloned().filter(|s| !s.is_empty()))
                .collect();
            build_array(cty, cells)
        })
        .collect();

    let batch = RecordBatch::try_new(schema.clone(), arrays).map_err(|e| e.to_string())?;
    Ok((schema, batch))
}

/// `ctx`へ登録済みのテーブルに対して`sql`を実行し、結果を
/// [`QueryResponse::Rows`]の形へ整形する(`run_olap`/`OlapCache::query`の
/// 共通の末尾処理)。
async fn execute_and_format(ctx: &SessionContext, sql: &str) -> Result<QueryResponse, String> {
    let df = ctx.sql(sql).await.map_err(|e| e.to_string())?;
    let batches = df.collect().await.map_err(|e| e.to_string())?;

    let columns: Vec<String> = batches
        .first()
        .map(|b| b.schema().fields().iter().map(|f| f.name().to_string()).collect())
        .unwrap_or_default();

    let opts = FormatOptions::default();
    let mut out_rows: Vec<Vec<Value>> = Vec::new();
    for batch in &batches {
        let formatters: Vec<ArrayFormatter<'_>> = batch
            .columns()
            .iter()
            .map(|c| ArrayFormatter::try_new(c.as_ref(), &opts))
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;

        for row in 0..batch.num_rows() {
            let cells: Vec<Value> = formatters.iter().map(|f| Value::Text(f.value(row).to_string())).collect();
            out_rows.push(cells);
        }
    }

    Ok(QueryResponse::Rows { columns, rows: out_rows })
}

/// DataFusion で OLAP クエリを実行する(キャッシュ無し、毎回全テーブルを
/// 行ストアからフル再構築する)。**正直な開示(2026-07-23)**: 大規模データ
/// では毎回のフル再構築がボトルネックになる——インクリメンタル同期版は
/// [`OlapCache`]を使うこと。この関数自体は後方互換のため残す。
pub async fn run_olap(engine: &QueryEngine, sql: &str) -> Result<QueryResponse, String> {
    let ctx = session_context();
    for (name, columns, rows) in engine.snapshot_tables() {
        if columns.is_empty() {
            continue;
        }
        let (schema, batch) = build_table_batch(&columns, &rows)?;
        let table = MemTable::try_new(schema, vec![vec![batch]]).map_err(|e| e.to_string())?;
        ctx.register_table(name.as_str(), Arc::new(table)).map_err(|e| e.to_string())?;
    }
    execute_and_format(&ctx, sql).await
}

/// HTAP列キャッシュ(TiDB/TiFlash方式のこのエコシステムなりの実装、
/// 2026-07-23新設、同日中にTiFlashのDelta Tree設計〈ベース列ストア+
/// デルタ行ストアをマージ、周期的にコンパクション〉を日英Web検索で
/// 調査の上で行単位へ再設計)。
///
/// `run_olap`が毎回全テーブルを行ストアからフル再構築するのに対し、
/// `OlapCache`は各テーブルについて「ベース」(Arrow列バッチ+その各行が
/// どのpkに対応するかの配列)と「変更されたpkの集合」を保持し、クエリ
/// のたびに:
/// 1. ベースから、変更されたpkに該当する行を`arrow::compute::filter`
///    (列指向のフィルタカーネル、文字列パースを伴わない軽量な操作)で除く。
/// 2. 変更されたpkだけを行ストアから読み直し(`QueryEngine::get_row`、
///    テーブル全体ではなく該当pkのみ)、小さな「デルタバッチ」を作る。
/// 3. フィルタ後のベース+デルタバッチを結合してクエリに使う(結合後の
///    ものを次回のベースとして採用=コンパクション)。
/// これにより、文字列→Arrow型付き配列への変換という重い処理
/// (`build_table_batch`)が必要になるのは「実際に変更された行の数」
/// だけになり、テーブルが大きいほど・変更が少ないほど効果が大きい
/// ——TiFlashが実践する「行ストアへの書き込みをデルタ層に貯め、
/// 列ストアとマージ」という核心思想を、単一プロセス内で実現したもの。
///
/// **正直な開示・スコープの限界**: (1) 単一プロセス内のみ——TiKV/
/// TiFlash間のような、ネットワーク越しの別ノードへの列レプリカ配置は
/// aruaru-distのRaftがまだ単一プロセス内実装(openraft統合待ち)の
/// ため範囲外。(2) 毎回コンパクション(フィルタ後ベース+デルタを即座に
/// 新ベースとして採用)する設計であり、TiFlashのような「デルタ層が
/// 一定サイズになるまで未コンパクションのまま複数バッチとして保持する」
/// 最適化は行っていない——正しさは保つが、書き込み1件ごとに軽量な
/// フィルタ処理が発生する点は今後の高頻度書き込み向け最適化の余地。
pub struct OlapCache {
    tables: parking_lot::RwLock<std::collections::HashMap<String, TableCache>>,
}

struct TableCache {
    schema: Arc<Schema>,
    /// ベース列バッチの各行が対応するpk(`base_batch`と同じ行順)。
    base_pks: Vec<Vec<u8>>,
    base_batch: RecordBatch,
}

impl OlapCache {
    pub fn new() -> Self {
        Self { tables: parking_lot::RwLock::new(std::collections::HashMap::new()) }
    }

    /// 現在キャッシュされているテーブル数(テスト・観測用)。
    pub fn cached_table_count(&self) -> usize {
        self.tables.read().len()
    }

    /// `table`が現在キャッシュに存在するか(テスト・観測用)。
    pub fn contains(&self, table: &str) -> bool {
        self.tables.read().contains_key(table)
    }

    /// テーブル全体を行ストアから読み直し、ベースを作り直す
    /// (初回・スキーマ変更時のみ通るパス)。
    fn rebuild_full(&self, engine: &QueryEngine, name: &str) -> Result<(), String> {
        let Some((columns, pks, rows)) = engine.snapshot_table(name) else {
            self.tables.write().remove(name);
            return Ok(());
        };
        engine.clear_olap_schema_dirty(name);
        let _ = engine.take_olap_delta_pks(name); // スキーマ再構築に吸収済み
        if columns.is_empty() {
            self.tables.write().remove(name);
            return Ok(());
        }
        let (schema, batch) = build_table_batch(&columns, &rows)?;
        self.tables.write().insert(name.to_string(), TableCache { schema, base_pks: pks, base_batch: batch });
        Ok(())
    }

    /// 変更されたpkだけをベースから除き、その現在値をデルタとして結合する
    /// (行単位インクリメンタル同期の核心パス)。
    fn rebuild_incremental(
        &self,
        engine: &QueryEngine,
        name: &str,
        delta_pks: std::collections::BTreeSet<Vec<u8>>,
    ) -> Result<(), String> {
        use datafusion::arrow::compute::filter_record_batch;

        let mut cache = self.tables.write();
        let Some(entry) = cache.get(name) else { return Ok(()) };

        // ベースの各行が、今回変更されたpkに該当するかどうかのマスク
        // (該当する=古い値なので除く、に該当しない=そのまま残す)。
        let keep_mask: BooleanArray = entry.base_pks.iter().map(|pk| Some(!delta_pks.contains(pk))).collect();
        let filtered = filter_record_batch(&entry.base_batch, &keep_mask).map_err(|e| e.to_string())?;
        let mut new_pks: Vec<Vec<u8>> =
            entry.base_pks.iter().zip(keep_mask.iter()).filter(|(_, keep)| keep.unwrap_or(false)).map(|(pk, _)| pk.clone()).collect();

        // 変更されたpkの「現在値」を1件ずつ読み直す(テーブル全体ではない)。
        // Noneは削除済みなので、デルタには含めない(=maskで除かれたまま復活しない)。
        let columns: Vec<(String, ColumnType)> = entry
            .schema
            .fields()
            .iter()
            .map(|f| (f.name().clone(), arrow_type_to_column_type(f.data_type())))
            .collect();
        let mut delta_rows: Vec<Vec<String>> = Vec::new();
        for pk in &delta_pks {
            if let Some(row) = engine.get_row(name, pk) {
                delta_rows.push(row);
                new_pks.push(pk.clone());
            }
        }

        let merged = if delta_rows.is_empty() {
            filtered
        } else {
            let (_, delta_batch) = build_table_batch(&columns, &delta_rows)?;
            datafusion::arrow::compute::concat_batches(&entry.schema, [&filtered, &delta_batch]).map_err(|e| e.to_string())?
        };

        let schema = entry.schema.clone();
        cache.insert(name.to_string(), TableCache { schema, base_pks: new_pks, base_batch: merged });
        Ok(())
    }

    /// 変更のあったテーブルだけ再構築(初回/スキーマ変更は全体、それ以外は
    /// 行単位デルタマージ)してキャッシュへ反映し、削除されたテーブルを
    /// キャッシュから除去する。
    fn refresh(&self, engine: &QueryEngine) -> Result<(), String> {
        let names = engine.table_names();
        self.tables.write().retain(|name, _| names.contains(name));
        for name in &names {
            let needs_full_rebuild = engine.is_olap_schema_dirty(name) || !self.contains(name);
            if needs_full_rebuild {
                self.rebuild_full(engine, name)?;
                continue;
            }
            let delta_pks = engine.take_olap_delta_pks(name);
            if delta_pks.is_empty() {
                continue; // 変更無し: 行ストアに一切触れずキャッシュを再利用
            }
            self.rebuild_incremental(engine, name, delta_pks)?;
        }
        Ok(())
    }

    /// インクリメンタル同期された列キャッシュ経由でOLAPクエリを実行する。
    pub async fn query(&self, engine: &QueryEngine, sql: &str) -> Result<QueryResponse, String> {
        self.refresh(engine)?;
        let ctx = session_context();
        for (name, entry) in self.tables.read().iter() {
            let table = MemTable::try_new(entry.schema.clone(), vec![vec![entry.base_batch.clone()]]).map_err(|e| e.to_string())?;
            ctx.register_table(name.as_str(), Arc::new(table)).map_err(|e| e.to_string())?;
        }
        execute_and_format(&ctx, sql).await
    }
}

impl Default for OlapCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Arrow DataType → catalog::ColumnType(逆変換、デルタ行の再構築時に
/// 元の列型へ揃えるために使う。`arrow_type`の対になる関数)。
fn arrow_type_to_column_type(ty: &DataType) -> ColumnType {
    match ty {
        DataType::Int64 => ColumnType::BigInt,
        DataType::Float64 => ColumnType::Float,
        DataType::Boolean => ColumnType::Bool,
        _ => ColumnType::Text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_olap_group_by_sum() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE orders (id INT, region TEXT, amount INT)")
            .unwrap();
        eng.execute("INSERT INTO orders (id, region, amount) VALUES (1, 'east', 100)")
            .unwrap();
        eng.execute("INSERT INTO orders (id, region, amount) VALUES (2, 'east', 50)")
            .unwrap();
        eng.execute("INSERT INTO orders (id, region, amount) VALUES (3, 'west', 70)")
            .unwrap();

        // catalog の型 (amount は INT) で登録されるため CAST 不要
        let resp = run_olap(
            &eng,
            "SELECT region, SUM(amount) AS total \
             FROM orders GROUP BY region ORDER BY region",
        )
        .await
        .unwrap();

        if let QueryResponse::Rows { columns, rows } = resp {
            assert_eq!(columns, vec!["region", "total"]);
            assert_eq!(rows.len(), 2);
            // east=150, west=70
            assert_eq!(rows[0][0], Value::Text("east".into()));
            assert_eq!(rows[0][1], Value::Text("150".into()));
            assert_eq!(rows[1][0], Value::Text("west".into()));
            assert_eq!(rows[1][1], Value::Text("70".into()));
        } else {
            panic!("expected rows");
        }
    }

    /// HTAP列キャッシュの核心特性: 一度クエリを実行した後、無関係な
    /// 別テーブルへの書き込みは、変更していないテーブルの列キャッシュ
    /// エントリ数(`cached_table_count`)を変えない——つまり変更の無い
    /// テーブルは行ストアから一切再構築されない、という実証。
    #[tokio::test]
    async fn olap_cache_reuses_unchanged_tables_and_rebuilds_only_dirty_ones() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE orders (id INT, amount INT)").unwrap();
        eng.execute("CREATE TABLE customers (id INT, name TEXT)").unwrap();
        eng.execute("INSERT INTO orders (id, amount) VALUES (1, 100)").unwrap();
        eng.execute("INSERT INTO customers (id, name) VALUES (1, 'alice')").unwrap();

        let cache = OlapCache::new();
        let resp = cache.query(&eng, "SELECT SUM(amount) AS total FROM orders").await.unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows[0][0], Value::Text("100".into()));
        } else {
            panic!("expected rows");
        }
        assert_eq!(cache.cached_table_count(), 2, "both tables queried at least once should be cached");
        assert!(!eng.has_pending_olap_delta("orders"));
        assert!(!eng.has_pending_olap_delta("customers"));

        // customersだけ更新 -> ordersのデルタは発生しないはず。
        eng.execute("INSERT INTO customers (id, name) VALUES (2, 'bob')").unwrap();
        assert!(!eng.has_pending_olap_delta("orders"), "unrelated table must not accumulate a delta");
        assert!(eng.has_pending_olap_delta("customers"));

        // 再クエリ: ordersの値は変わらず正しく返る(キャッシュ再利用でも
        // 結果が壊れないことの確認)、かつcustomersの新しい行も反映される
        // (デルタがあったテーブルは正しく再構築されることの確認)。
        let resp = cache
            .query(&eng, "SELECT COUNT(*) AS n FROM customers")
            .await
            .unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows[0][0], Value::Text("2".into()), "customers cache must reflect the new row");
        } else {
            panic!("expected rows");
        }
        assert!(!eng.has_pending_olap_delta("customers"), "delta must be cleared after rebuild");
    }

    /// 行単位デルタマージの正しさ: 既存行の更新・削除・新規追加が全て
    /// 正しく反映され、かつ更新前の古い値がベースに残って二重集計
    /// されないことを実証する(TiFlashのDelta Tree設計から借用した
    /// 「ベースからフィルタで除いてデルタと結合」の核心的な正しさ検証)。
    #[tokio::test]
    async fn olap_cache_incremental_merge_handles_update_delete_and_insert_correctly() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE items (id INT, qty INT)").unwrap();
        eng.execute("INSERT INTO items (id, qty) VALUES (1, 10)").unwrap();
        eng.execute("INSERT INTO items (id, qty) VALUES (2, 20)").unwrap();
        eng.execute("INSERT INTO items (id, qty) VALUES (3, 30)").unwrap();

        let cache = OlapCache::new();
        let resp = cache.query(&eng, "SELECT SUM(qty) AS total FROM items").await.unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows[0][0], Value::Text("60".into()));
        } else {
            panic!("expected rows");
        }

        // id=1を更新(10->99)・id=2を削除・id=4を新規追加。
        eng.execute("UPDATE items SET qty = 99 WHERE id = 1").unwrap();
        eng.execute("DELETE FROM items WHERE id = 2").unwrap();
        eng.execute("INSERT INTO items (id, qty) VALUES (4, 40)").unwrap();

        // 正しい合計: 99(更新後) + 30(無変更) + 40(新規) = 169。
        // 古い値(id=1の10、削除されたid=2の20)が残って二重集計されて
        // いないこと、ベースのフィルタ+デルタ結合が正しく機能している
        // ことの直接証明。
        let resp = cache.query(&eng, "SELECT SUM(qty) AS total FROM items").await.unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows[0][0], Value::Text("169".into()));
        } else {
            panic!("expected rows");
        }

        let resp = cache.query(&eng, "SELECT COUNT(*) AS n FROM items").await.unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows[0][0], Value::Text("3".into()), "id=1,3,4 should remain, id=2 deleted");
        } else {
            panic!("expected rows");
        }
    }
}
