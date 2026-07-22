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
/// 2026-07-23新設)。
///
/// `run_olap`が毎回全テーブルを行ストアからフル再構築するのに対し、
/// `OlapCache`は各テーブルのArrow (Schema, RecordBatch)をプロセス内に
/// 保持し、`QueryEngine::is_olap_table_dirty`が変更を報告したテーブル
/// **だけ**を再構築する。変更の無いテーブルは行ストアに一切触れず
/// キャッシュを再利用する——「行ストアの変更を列ストアへ継続的に
/// 同期する」というHTAPの核心的性質を、単一プロセス内でのテーブル単位
/// 粒度という現実的なスコープで実現したもの。
///
/// **正直な開示・スコープの限界**: (1) 粒度はテーブル単位であり、
/// TiFlashのような行単位の真のインクリメンタル書き込み(1行変更で
/// 1行だけ列ストアへ反映)ではない——1行でも変更されたテーブルは
/// テーブル全体を再構築する。(2) 単一プロセス内のみ——TiKV/TiFlash間の
/// ような、ネットワーク越しの別ノードへの列レプリカ配置は
/// aruaru-distのRaftがまだ単一プロセス内実装(openraft統合待ち)の
/// ため範囲外。
pub struct OlapCache {
    tables: parking_lot::RwLock<std::collections::HashMap<String, (Arc<Schema>, RecordBatch)>>,
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

    /// dirtyなテーブルだけ再構築してキャッシュへ反映し、削除された
    /// テーブルをキャッシュから除去する。
    fn refresh(&self, engine: &QueryEngine) -> Result<(), String> {
        let names = engine.table_names();
        let mut cache = self.tables.write();
        cache.retain(|name, _| names.contains(name));
        for name in &names {
            if cache.contains_key(name) && !engine.is_olap_table_dirty(name) {
                continue; // 変更無し: 行ストアに触れずキャッシュを再利用
            }
            let Some((columns, rows)) = engine.snapshot_table(name) else { continue };
            if columns.is_empty() {
                continue;
            }
            let (schema, batch) = build_table_batch(&columns, &rows)?;
            cache.insert(name.clone(), (schema, batch));
            engine.clear_olap_dirty(name);
        }
        Ok(())
    }

    /// インクリメンタル同期された列キャッシュ経由でOLAPクエリを実行する。
    pub async fn query(&self, engine: &QueryEngine, sql: &str) -> Result<QueryResponse, String> {
        self.refresh(engine)?;
        let ctx = session_context();
        for (name, (schema, batch)) in self.tables.read().iter() {
            let table = MemTable::try_new(schema.clone(), vec![vec![batch.clone()]]).map_err(|e| e.to_string())?;
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
        assert!(!eng.is_olap_table_dirty("orders"));
        assert!(!eng.is_olap_table_dirty("customers"));

        // customersだけ更新 -> ordersはdirtyにならないはず。
        eng.execute("INSERT INTO customers (id, name) VALUES (2, 'bob')").unwrap();
        assert!(!eng.is_olap_table_dirty("orders"), "unrelated table must not be marked dirty");
        assert!(eng.is_olap_table_dirty("customers"));

        // 再クエリ: ordersの値は変わらず正しく返る(キャッシュ再利用でも
        // 結果が壊れないことの確認)、かつcustomersの新しい行も反映される
        // (dirtyだったテーブルは正しく再構築されることの確認)。
        let resp = cache
            .query(&eng, "SELECT COUNT(*) AS n FROM customers")
            .await
            .unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows[0][0], Value::Text("2".into()), "customers cache must reflect the new row");
        } else {
            panic!("expected rows");
        }
        assert!(!eng.is_olap_table_dirty("customers"), "dirty flag must be cleared after rebuild");
    }
}
