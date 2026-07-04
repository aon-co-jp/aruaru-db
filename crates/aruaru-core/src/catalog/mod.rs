//! カタログ: テーブル・スキーマ・列のメタデータ

use serde::{Deserialize, Serialize};

/// テーブル ID
pub type TableId = u32;

/// 列の型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnType {
    Int,
    BigInt,
    Text,
    Bool,
    Float,
    Bytes,
    Timestamp,
}

impl ColumnType {
    /// PostgreSQL の型名から推定
    pub fn from_sql(sql_type: &str) -> Self {
        match sql_type.to_uppercase().as_str() {
            "INT" | "INTEGER" | "INT4" => ColumnType::Int,
            "BIGINT" | "INT8" | "SERIAL" | "BIGSERIAL" => ColumnType::BigInt,
            "BOOL" | "BOOLEAN" => ColumnType::Bool,
            "FLOAT" | "REAL" | "DOUBLE" | "NUMERIC" => ColumnType::Float,
            "BYTEA" | "BLOB" => ColumnType::Bytes,
            "TIMESTAMP" | "TIMESTAMPTZ" | "DATETIME" => ColumnType::Timestamp,
            _ => ColumnType::Text, // TEXT / VARCHAR / その他
        }
    }

    /// PostgreSQL ワイヤ用の型名
    pub fn pg_type_name(&self) -> &'static str {
        match self {
            ColumnType::Int => "int4",
            ColumnType::BigInt => "int8",
            ColumnType::Text => "text",
            ColumnType::Bool => "bool",
            ColumnType::Float => "float8",
            ColumnType::Bytes => "bytea",
            ColumnType::Timestamp => "timestamp",
        }
    }
}

/// 列定義
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub ty: ColumnType,
    pub nullable: bool,
    pub primary_key: bool,
}

impl Column {
    pub fn new(name: impl Into<String>, ty: ColumnType) -> Self {
        Self {
            name: name.into(),
            ty,
            nullable: true,
            primary_key: false,
        }
    }

    pub fn primary_key(mut self) -> Self {
        self.primary_key = true;
        self.nullable = false;
        self
    }

    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }
}

/// テーブルスキーマ
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schema {
    pub table_id: TableId,
    pub name: String,
    pub columns: Vec<Column>,
}

impl Schema {
    pub fn new(table_id: TableId, name: impl Into<String>, columns: Vec<Column>) -> Self {
        Self {
            table_id,
            name: name.into(),
            columns,
        }
    }

    /// 主キー列のインデックス (なければ 0 列目を PK とみなす)
    pub fn pk_index(&self) -> usize {
        self.columns
            .iter()
            .position(|c| c.primary_key)
            .unwrap_or(0)
    }

    /// 列名からインデックスを取得
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }
}

/// カタログエラー
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("table already exists: {0}")]
    TableExists(String),
    #[error("table not found: {0}")]
    TableNotFound(String),
    #[error("column not found: {0}")]
    ColumnNotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column_type_from_sql() {
        assert_eq!(ColumnType::from_sql("INTEGER"), ColumnType::Int);
        assert_eq!(ColumnType::from_sql("BIGSERIAL"), ColumnType::BigInt);
        assert_eq!(ColumnType::from_sql("VARCHAR"), ColumnType::Text);
        assert_eq!(ColumnType::from_sql("BOOLEAN"), ColumnType::Bool);
    }

    #[test]
    fn test_schema_pk() {
        let schema = Schema::new(
            1,
            "users",
            vec![
                Column::new("id", ColumnType::BigInt).primary_key(),
                Column::new("name", ColumnType::Text),
            ],
        );
        assert_eq!(schema.pk_index(), 0);
        assert_eq!(schema.column_index("name"), Some(1));
    }
}
