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

/// DataFusion で OLAP クエリを実行する。
pub async fn run_olap(engine: &QueryEngine, sql: &str) -> Result<QueryResponse, String> {
    // 並列度 = 利用可能 CPU 数 (パーティション並列実行に使われる)
    let parallelism = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let config = SessionConfig::new().with_target_partitions(parallelism);
    let ctx = SessionContext::new_with_config(config);

    // 全テーブルを Arrow MemTable として登録 (catalog の型で構築)
    for (name, columns, rows) in engine.snapshot_tables() {
        if columns.is_empty() {
            continue;
        }
        let fields: Vec<Field> = columns
            .iter()
            .map(|(cname, cty)| Field::new(cname, arrow_type(cty), true))
            .collect();
        let schema = Arc::new(Schema::new(fields));

        // 列ごとに型付き配列を構築
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
        let table = MemTable::try_new(schema, vec![vec![batch]]).map_err(|e| e.to_string())?;
        ctx.register_table(name.as_str(), Arc::new(table))
            .map_err(|e| e.to_string())?;
    }

    // SQL 実行 (DataFusion の論理→物理プラン最適化 + 並列実行)
    let df = ctx.sql(sql).await.map_err(|e| e.to_string())?;
    let batches = df.collect().await.map_err(|e| e.to_string())?;

    // RecordBatch 群を文字列の行に変換
    let columns: Vec<String> = batches
        .first()
        .map(|b| {
            b.schema()
                .fields()
                .iter()
                .map(|f| f.name().to_string())
                .collect()
        })
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
            let cells: Vec<Value> = formatters
                .iter()
                .map(|f| Value::Text(f.value(row).to_string()))
                .collect();
            out_rows.push(cells);
        }
    }

    Ok(QueryResponse::Rows {
        columns,
        rows: out_rows,
    })
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
}
