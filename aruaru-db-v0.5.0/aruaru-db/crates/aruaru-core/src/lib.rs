//! aruaru-core: バージョン管理 (Git-on-SQL) + カタログ + (v0.3.x) ストレージエンジン
//!
//! # v0.3 実装状況
//! - `version::`  ✅ Prolly Tree + ブランチ/コミット/diff (実装済み)
//! - `catalog::`  ✅ スキーマ・列・テーブルメタデータ (実装済み)
//! - `storage::`  🚧 fjall LSM 行ストア + 列ストア (v0.3.x で接続予定)
//!
//! ストレージエンジン (fjall) 接続前の現段階では、上位の QueryEngine が
//! 行データを保持し、コミット時に version::prolly へスナップショットする。

#![forbid(unsafe_code)]
#![warn(rust_2018_idioms)]

pub mod catalog;
pub mod storage;
pub mod version;

pub use catalog::{Column, ColumnType, Schema, TableId};
pub use storage::PersistentStore;
pub use version::{Branch, Commit, CommitId, Diff, VersionController};

/// aruaru-core のエラー型
#[derive(Debug, thiserror::Error)]
pub enum AruaruError {
    #[error("version error: {0}")]
    Version(#[from] version::VersionError),

    #[error("catalog error: {0}")]
    Catalog(#[from] catalog::CatalogError),

    #[error("storage error: {0}")]
    Storage(#[from] storage::StorageError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AruaruError>;
