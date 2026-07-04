//! aruaru-db-rust エラー型

use thiserror::Error;

/// aruaru-db-rust のエラー
#[derive(Debug, Error)]
pub enum AruaruError {
    #[error("connection failed: {0}")]
    Connect(#[source] tokio_postgres::Error),

    #[error("query failed: {0}")]
    Query(#[source] tokio_postgres::Error),

    #[error("connection pool error: {0}")]
    Pool(String),

    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    #[error("row mapping error: {0}")]
    RowMapping(String),
}

pub type Result<T> = std::result::Result<T, AruaruError>;
