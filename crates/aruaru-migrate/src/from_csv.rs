//! CSV / NDJSON 移行
//!
//! ローカル CSV ファイルをストリーミング読み込みし、
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
    tracing::info!(path, "CSV migration: starting");

    let table_name = table_name_from_path(path);
    let (headers, rows) = read_csv_rows(path)?;

    progress_cb(MigrateProgress {
        table: table_name.clone(),
        rows_total: Some(rows.len() as u64),
        rows_done: 0,
        bytes_done: 0,
        elapsed_ms: 0,
        status: MigrateStatus::Running,
    });

    let target = TargetClient::connect(&config.target_uri).await?;
    if !headers.is_empty() {
        target.ensure_table(&table_name, &headers).await?;
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
        .unwrap_or_else(|| "csv_import".to_string())
}

/// CSV ファイルを (ヘッダ, 各行の文字列) として読み込む
fn read_csv_rows(path: &str) -> anyhow::Result<(Vec<String>, Vec<Vec<String>>)> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)?;
    let headers: Vec<String> = reader.headers()?.iter().map(|s| s.to_string()).collect();
    let mut rows = Vec::new();
    for result in reader.records() {
        let record = result?;
        rows.push(record.iter().map(|s| s.to_string()).collect());
    }
    Ok((headers, rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_name_from_path() {
        assert_eq!(table_name_from_path("/tmp/users.csv"), "users");
        assert_eq!(table_name_from_path("data/orders.csv"), "orders");
        assert_eq!(table_name_from_path(""), "csv_import");
    }

    #[test]
    fn test_read_csv_rows() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("aruaru-migrate-test-{}.csv", std::process::id()));
        std::fs::write(&path, "id,name\n1,alice\n2,bob\n").unwrap();

        let (headers, rows) = read_csv_rows(path.to_str().unwrap()).unwrap();
        assert_eq!(headers, vec!["id".to_string(), "name".to_string()]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["1".to_string(), "alice".to_string()]);
        assert_eq!(rows[1], vec!["2".to_string(), "bob".to_string()]);

        let _ = std::fs::remove_file(&path);
    }
}
