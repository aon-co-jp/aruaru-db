//! スキーマ変換: MySQL / Snowflake → aruaru-DB PostgreSQL 互換 DDL
pub fn mysql_to_aruaru(ddl: &str) -> String {
    ddl.replace("INT UNSIGNED", "BIGINT")
       .replace("TINYINT(1)", "BOOLEAN")
       .replace("DATETIME", "TIMESTAMP")
       .replace("`", "\"")
}
