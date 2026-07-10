//! Parquet / Snowflake エクスポート移行
//!
//! Snowflake からの移行は `COPY INTO ... FILE_FORMAT = (TYPE = PARQUET)` 等で
//! 書き出したファイルを前提とする (`SourceKind::Snowflake` も本経路を使う)。
//! ローカル Parquet ファイルを読み込み、全列を text 表現へ変換したうえで
//! `crate::target::TargetClient` 経由で aruaru-DB へ INSERT する。
use crate::target::TargetClient;
use crate::{MigrateConfig, MigrateProgress, MigrateStatus};

pub async fn migrate(
    config: &MigrateConfig,
    progress_cb: impl Fn(MigrateProgress) + Send + 'static,
) -> anyhow::Result<()> {
    let path = config
        .source_uri
        .strip_prefix("file://")
        .unwrap_or(&config.source_uri);
    tracing::info!(path, "Parquet/Snowflake migration: starting");

    let table_name = table_name_from_path(path);
    let (columns, rows) = read_parquet_rows(path)?;

    progress_cb(MigrateProgress {
        table: table_name.clone(),
        rows_total: Some(rows.len() as u64),
        rows_done: 0,
        bytes_done: 0,
        elapsed_ms: 0,
        status: MigrateStatus::Running,
    });

    let target = TargetClient::connect(&config.target_uri).await?;
    if !columns.is_empty() {
        target.ensure_table(&table_name, &columns).await?;
    }
    let done = target.insert_rows(&table_name, &rows).await;

    progress_cb(MigrateProgress {
        table: table_name,
        rows_total: Some(rows.len() as u64),
        rows_done: done as u64,
        bytes_done: 0,
        elapsed_ms: 0,
        status: MigrateStatus::Done,
    });

    Ok(())
}

/// ファイルパスからテーブル名を推定する (拡張子を除いたファイル名)
fn table_name_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "parquet_import".to_string())
}

/// Parquet ファイルを (列名, 各行の text 表現) として読み込む。
/// 任意の Arrow 型を `arrow_cast::display` で text 表現へ変換する。
fn read_parquet_rows(path: &str) -> anyhow::Result<(Vec<String>, Vec<Vec<String>>)> {
    use arrow::array::Array;
    use arrow_cast::display::array_value_to_string;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    let file = std::fs::File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let columns: Vec<String> = builder
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect();
    let reader = builder.build()?;

    let mut rows: Vec<Vec<String>> = Vec::new();
    for batch in reader {
        let batch = batch?;
        for row_idx in 0..batch.num_rows() {
            let mut row = Vec::with_capacity(columns.len());
            for col_idx in 0..batch.num_columns() {
                let array = batch.column(col_idx);
                let value = if array.is_null(row_idx) {
                    String::new()
                } else {
                    array_value_to_string(array, row_idx).unwrap_or_default()
                };
                row.push(value);
            }
            rows.push(row);
        }
    }
    Ok((columns, rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_name_from_path() {
        assert_eq!(table_name_from_path("/tmp/export.parquet"), "export");
        assert_eq!(table_name_from_path("data/orders.snappy.parquet"), "orders.snappy");
        assert_eq!(table_name_from_path(""), "parquet_import");
    }

    #[test]
    fn test_read_parquet_rows_round_trip() {
        use arrow::array::{ArrayRef, Int32Array, StringArray};
        use arrow::datatypes::{DataType, Field, Schema};
        use arrow::record_batch::RecordBatch;
        use parquet::arrow::ArrowWriter;
        use std::sync::Arc;

        let dir = std::env::temp_dir();
        let path = dir.join(format!("aruaru-migrate-test-{}.parquet", std::process::id()));

        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, true),
        ]));
        let id_array: ArrayRef = Arc::new(Int32Array::from(vec![1, 2]));
        let name_array: ArrayRef = Arc::new(StringArray::from(vec![Some("alice"), Some("bob")]));
        let batch = RecordBatch::try_new(schema.clone(), vec![id_array, name_array]).unwrap();

        let file = std::fs::File::create(&path).unwrap();
        let mut writer = ArrowWriter::try_new(file, schema, None).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();

        let (columns, rows) = read_parquet_rows(path.to_str().unwrap()).unwrap();
        assert_eq!(columns, vec!["id".to_string(), "name".to_string()]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["1".to_string(), "alice".to_string()]);
        assert_eq!(rows[1], vec!["2".to_string(), "bob".to_string()]);

        let _ = std::fs::remove_file(&path);
    }
}
