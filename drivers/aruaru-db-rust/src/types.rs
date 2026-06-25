//! aruaru-db-rust 共通データ型

use tokio_postgres::Row;

/// コミットログの1エントリ
#[derive(Debug, Clone)]
pub struct CommitEntry {
    pub id:        String,
    pub short_id:  String,
    pub author:    String,
    pub message:   String,
    pub timestamp: String,
    pub root_hash: String,
}

impl CommitEntry {
    pub(crate) fn from_row(row: &Row) -> Self {
        Self {
            id:        row.try_get("id").unwrap_or_default(),
            short_id:  row.try_get("short_id").unwrap_or_default(),
            author:    row.try_get("author").unwrap_or_default(),
            message:   row.try_get("message").unwrap_or_default(),
            timestamp: row.try_get("timestamp").unwrap_or_default(),
            root_hash: row.try_get("root_hash").unwrap_or_default(),
        }
    }
}

/// 2ブランチ間の差分統計
#[derive(Debug, Clone, Default)]
pub struct DiffStat {
    pub added:    i64,
    pub removed:  i64,
    pub modified: i64,
}
