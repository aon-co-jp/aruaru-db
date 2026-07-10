//! 移行先 (aruaru-DB / PostgreSQL ワイヤ互換) 向け DDL/DML 文字列組み立て。
//!
//! aruaru-query の SQL サブセット (`CREATE TABLE t (col1, col2, ...)`) は
//! 型なし・全列 TEXT 前提のため、ここで生成する DDL も同じ前提に合わせる。
//! ネットワーク接続を伴わない純粋関数として切り出し、単体テストで検証する。

/// 値を SQL テキストリテラルとして安全にクォートする
/// (シングルクォートを `''` に二重化する)。
pub fn quote_value(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// 識別子 (テーブル名・列名) を二重引用符でクォートする。
/// 埋め込み二重引用符は `""` に二重化する。
pub fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// `CREATE TABLE IF NOT EXISTS <table> (<col1>, <col2>, ...)` を組み立てる
pub fn build_create_table_sql(table: &str, columns: &[String]) -> String {
    let cols = columns
        .iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    format!("CREATE TABLE IF NOT EXISTS {} ({cols})", quote_ident(table))
}

/// `INSERT INTO <table> VALUES (<v1>, <v2>, ...)` を組み立てる
pub fn build_insert_sql(table: &str, row: &[String]) -> String {
    let values = row
        .iter()
        .map(|v| quote_value(v))
        .collect::<Vec<_>>()
        .join(", ");
    format!("INSERT INTO {} VALUES ({values})", quote_ident(table))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quote_value_escapes_single_quote() {
        assert_eq!(quote_value("it's"), "'it''s'");
    }

    #[test]
    fn test_quote_ident_escapes_double_quote() {
        assert_eq!(quote_ident("weird\"name"), "\"weird\"\"name\"");
    }

    #[test]
    fn test_build_create_table_sql() {
        let sql = build_create_table_sql(
            "users",
            &["id".to_string(), "name".to_string()],
        );
        assert_eq!(sql, "CREATE TABLE IF NOT EXISTS \"users\" (\"id\", \"name\")");
    }

    #[test]
    fn test_build_insert_sql() {
        let sql = build_insert_sql(
            "users",
            &["1".to_string(), "it's a name".to_string()],
        );
        assert_eq!(sql, "INSERT INTO \"users\" VALUES ('1', 'it''s a name')");
    }

    #[test]
    fn test_build_insert_sql_rejects_injection_by_quoting_not_stripping() {
        // インジェクション試行の文字列もリテラルとして安全にクォートされ、
        // 文の外へ脱出できないことを確認する
        let payload = "'; DROP TABLE users; --".to_string();
        let sql = build_insert_sql("t", &[payload]);
        assert_eq!(sql, "INSERT INTO \"t\" VALUES ('''; DROP TABLE users; --')");
    }
}
