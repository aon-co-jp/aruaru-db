//! 対応DBレジストリの型定義

use serde::{Deserialize, Serialize};

/// DB の分類
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Category {
    Relational,
    Document,
    KeyValue,
    WideColumn,
    Graph,
    TimeSeries,
    Search,
    Vector,
}

/// 接続ワイヤプロトコル
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wire {
    Postgres,    // PostgreSQL ワイヤ (aruaru が実接続できる)
    MySQL,       // MySQL ワイヤ
    Mongo,       // MongoDB ワイヤ
    Redis,       // RESP
    Cql,         // Cassandra CQL
    Tds,         // SQL Server TDS
    Oracle,      // Oracle Net
    Http,        // REST/HTTP API
    File,        // ファイル (CSV/Parquet/SQLite 等)
    Proprietary, // 独自/専用ドライバ
    Other,       // その他
}

/// 対応段階 (5 段階)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    /// 一般提供。ネイティブに完全対応
    Ga,
    /// ベータ。実装済みだが検証中
    Beta,
    /// PostgreSQL ワイヤ互換として接続可能 (実接続ドライバ経由)
    PgCompatible,
    /// 読み取り専用で対応 (取り込み・参照のみ)
    ReadOnly,
    /// 計画中 (レジストリに登録済み・未接続)
    Planned,
}

impl Status {
    pub fn label(&self) -> &'static str {
        match self {
            Status::Ga => "GA",
            Status::Beta => "Beta",
            Status::PgCompatible => "PG互換接続可",
            Status::ReadOnly => "読取専用",
            Status::Planned => "計画中",
        }
    }
    /// 何らかの形で実接続/取り込みが可能か
    pub fn is_connectable(&self) -> bool {
        !matches!(self, Status::Planned)
    }
}

/// 移行(お引越し)経路
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrateVia {
    PgWire,    // PostgreSQL ワイヤで直接取り込み
    MySqlWire, // MySQL ワイヤで直接取り込み
    Csv,       // CSV エクスポート経由
    Parquet,   // Parquet エクスポート経由
    Dump,      // ネイティブ dump ファイル経由
    Api,       // REST/専用 API 経由
    File,      // ファイルを直接読む
    None,      // 移行経路なし
}

/// バックアップ対応度
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupSupport {
    Full,     // フル + 増分
    Snapshot, // スナップショットのみ
    None,     // 非対応
}

impl BackupSupport {
    /// 移行経路からバックアップ対応度を推定
    pub fn from_migrate(m: MigrateVia) -> Self {
        match m {
            MigrateVia::PgWire | MigrateVia::MySqlWire => BackupSupport::Full,
            MigrateVia::Csv | MigrateVia::Parquet | MigrateVia::Dump | MigrateVia::File => {
                BackupSupport::Snapshot
            }
            MigrateVia::Api => BackupSupport::Snapshot,
            MigrateVia::None => BackupSupport::None,
        }
    }
}

/// レジストリ 1 エントリ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbEntry {
    pub id: String,
    pub name: String,
    pub category: Category,
    pub wire: Wire,
    pub status: Status,
    pub migrate: MigrateVia,
    pub backup: BackupSupport,
    /// DB-Engines 等のランキング順位 (クロールで更新)
    pub rank: Option<u32>,
    /// 人気度スコア (クロールで更新)
    pub score: Option<f64>,
    /// 最終更新時刻 (RFC3339, クロール時刻)
    pub updated_at: Option<String>,
}
