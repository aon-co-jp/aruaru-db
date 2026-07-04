//! 軽量 SQL サブセットパーサ
//!
//! 完全な SQL パーサではなく、v0.3 で pgwire を動かすために必要な
//! 文を分類・抽出する最小実装。コンパイル安定性を最優先。

use aruaru_core::catalog::ColumnType;

/// 型付き列定義
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDef {
    pub name: String,
    pub ty: ColumnType,
}

/// パース結果の文
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    CreateTable {
        table: String,
        columns: Vec<ColumnDef>,
    },
    Insert {
        table: String,
        columns: Vec<String>,
        values: Vec<String>,
    },
    Select {
        table: String,
        /// None = SELECT *
        columns: Option<Vec<String>>,
        /// WHERE col = 'value'
        filter: Option<(String, String)>,
    },
    Delete {
        table: String,
        filter: Option<(String, String)>,
    },
    Update {
        table: String,
        /// SET col = 'value' (単一列)
        set: (String, String),
        filter: Option<(String, String)>,
    },
    DropTable {
        table: String,
    },
    /// トランザクション開始 (BEGIN / START TRANSACTION)
    Begin,
    /// トランザクションコミット (COMMIT)
    TxnCommit,
    /// トランザクションロールバック (ROLLBACK)
    Rollback,
    /// Git-on-SQL 関数呼び出し: SELECT aruaru_xxx('arg')
    AruaruFn {
        name: String,
        arg: Option<String>,
    },
    /// SELECT * FROM aruaru_log
    AruaruLog {
        limit: Option<usize>,
    },
}

/// SQL 文字列をパースする
pub fn parse(sql: &str) -> Result<Statement, String> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let upper = trimmed.to_uppercase();

    // ── Git-on-SQL 関数 ──────────────────────────────────────
    if upper.contains("ARUARU_LOG") {
        let limit = extract_limit(&upper);
        return Ok(Statement::AruaruLog { limit });
    }
    for fname in ["aruaru_branch", "aruaru_checkout", "aruaru_commit", "aruaru_merge"] {
        if let Some(arg) = extract_fn_arg(trimmed, fname) {
            return Ok(Statement::AruaruFn {
                name: fname.to_string(),
                arg,
            });
        }
    }

    // ── CREATE TABLE ─────────────────────────────────────────
    if upper.starts_with("CREATE TABLE") {
        return parse_create_table(trimmed);
    }

    // ── INSERT ───────────────────────────────────────────────
    if upper.starts_with("INSERT INTO") {
        return parse_insert(trimmed);
    }

    // ── SELECT ───────────────────────────────────────────────
    if upper.starts_with("SELECT") {
        return parse_select(trimmed);
    }

    // ── DELETE ───────────────────────────────────────────────
    if upper.starts_with("DELETE FROM") {
        return parse_delete(trimmed);
    }

    // ── UPDATE ───────────────────────────────────────────────
    if upper.starts_with("UPDATE") {
        return parse_update(trimmed);
    }

    // ── トランザクション制御 ──────────────────────────────────
    if upper == "BEGIN" || upper.starts_with("BEGIN ") || upper.starts_with("START TRANSACTION") {
        return Ok(Statement::Begin);
    }
    if upper == "COMMIT" || upper.starts_with("COMMIT ") || upper == "END" {
        return Ok(Statement::TxnCommit);
    }
    if upper == "ROLLBACK" || upper.starts_with("ROLLBACK ") || upper == "ABORT" {
        return Ok(Statement::Rollback);
    }

    // ── DROP TABLE ───────────────────────────────────────────
    if upper.starts_with("DROP TABLE") {
        return parse_drop(trimmed);
    }

    Err(format!("unsupported statement: {}", trimmed))
}

/// `fname('arg')` の引数を抽出。マッチしなければ None。
fn extract_fn_arg(sql: &str, fname: &str) -> Option<Option<String>> {
    let lower = sql.to_lowercase();
    let pos = lower.find(fname)?;
    let after = &sql[pos + fname.len()..];
    let open = after.find('(')?;
    let close = after.find(')')?;
    let inside = after[open + 1..close].trim();
    if inside.is_empty() {
        Some(None)
    } else {
        // 'arg' のクォートを除去
        let arg = inside.trim_matches(|c| c == '\'' || c == '"').to_string();
        Some(Some(arg))
    }
}

fn extract_limit(upper: &str) -> Option<usize> {
    let pos = upper.find("LIMIT")?;
    upper[pos + 5..]
        .trim()
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn parse_create_table(sql: &str) -> Result<Statement, String> {
    // CREATE TABLE [IF NOT EXISTS] name (col1 TYPE, col2 TYPE, ...)
    let after = sql[12..].trim(); // "CREATE TABLE".len() == 12
    let after = after
        .strip_prefix("IF NOT EXISTS")
        .or_else(|| after.strip_prefix("if not exists"))
        .map(|s| s.trim())
        .unwrap_or(after);

    let paren = after
        .find('(')
        .ok_or_else(|| "CREATE TABLE: missing '('".to_string())?;
    let table = after[..paren].trim().to_string();
    let close = after
        .rfind(')')
        .ok_or_else(|| "CREATE TABLE: missing ')'".to_string())?;
    let cols_str = &after[paren + 1..close];

    let columns = cols_str
        .split(',')
        .filter_map(|c| {
            // "name TYPE ..." → 列名 + 型
            let mut it = c.trim().split_whitespace();
            let name = it.next().unwrap_or("").to_string();
            if name.is_empty() {
                return None;
            }
            // 型トークン (無ければ TEXT 扱い)
            let ty = it
                .next()
                .map(ColumnType::from_sql)
                .unwrap_or(ColumnType::Text);
            Some(ColumnDef { name, ty })
        })
        .collect();

    Ok(Statement::CreateTable { table, columns })
}

fn parse_insert(sql: &str) -> Result<Statement, String> {
    // INSERT INTO name (c1, c2) VALUES (v1, v2)
    let after = sql[11..].trim(); // "INSERT INTO".len() == 11
    let paren = after
        .find('(')
        .ok_or_else(|| "INSERT: missing column list".to_string())?;
    let table = after[..paren].trim().to_string();

    let cols_close = after
        .find(')')
        .ok_or_else(|| "INSERT: missing ')'".to_string())?;
    let columns: Vec<String> = after[paren + 1..cols_close]
        .split(',')
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();

    let values_kw = after.to_uppercase().find("VALUES").ok_or_else(|| {
        "INSERT: missing VALUES".to_string()
    })?;
    let values_part = &after[values_kw + 6..];
    let vopen = values_part
        .find('(')
        .ok_or_else(|| "INSERT: missing values '('".to_string())?;
    let vclose = values_part
        .rfind(')')
        .ok_or_else(|| "INSERT: missing values ')'".to_string())?;
    let values: Vec<String> = values_part[vopen + 1..vclose]
        .split(',')
        .map(|v| v.trim().trim_matches(|c| c == '\'' || c == '"').to_string())
        .collect();

    if columns.len() != values.len() {
        return Err(format!(
            "INSERT: {} columns but {} values",
            columns.len(),
            values.len()
        ));
    }

    Ok(Statement::Insert {
        table,
        columns,
        values,
    })
}

fn parse_select(sql: &str) -> Result<Statement, String> {
    // SELECT cols FROM table [WHERE col = 'val']
    let upper = sql.to_uppercase();
    let from_pos = upper
        .find(" FROM ")
        .ok_or_else(|| "SELECT: missing FROM".to_string())?;
    let cols_str = sql[6..from_pos].trim();
    let columns = if cols_str == "*" {
        None
    } else {
        Some(cols_str.split(',').map(|c| c.trim().to_string()).collect())
    };

    let after_from = &sql[from_pos + 6..];
    let (table, filter) = if let Some(where_pos) = after_from.to_uppercase().find(" WHERE ") {
        let table = after_from[..where_pos].trim().to_string();
        let cond = after_from[where_pos + 7..].trim();
        let filter = parse_where(cond);
        (table, filter)
    } else {
        (after_from.trim().to_string(), None)
    };

    Ok(Statement::Select {
        table,
        columns,
        filter,
    })
}

/// `col = 'value'` をパース
fn parse_where(cond: &str) -> Option<(String, String)> {
    let eq = cond.find('=')?;
    let col = cond[..eq].trim().to_string();
    let val = cond[eq + 1..]
        .trim()
        .trim_matches(|c| c == '\'' || c == '"')
        .to_string();
    Some((col, val))
}

fn parse_delete(sql: &str) -> Result<Statement, String> {
    // DELETE FROM t [WHERE col = 'v']
    let after = sql[11..].trim(); // "DELETE FROM".len() == 11
    let (table, filter) = if let Some(wp) = after.to_uppercase().find(" WHERE ") {
        (after[..wp].trim().to_string(), parse_where(after[wp + 7..].trim()))
    } else {
        (after.trim().to_string(), None)
    };
    if table.is_empty() {
        return Err("DELETE: missing table".to_string());
    }
    Ok(Statement::Delete { table, filter })
}

fn parse_update(sql: &str) -> Result<Statement, String> {
    // UPDATE t SET col = 'v' [WHERE col2 = 'v2']
    let after = sql[6..].trim(); // "UPDATE".len() == 6
    let set_pos = after
        .to_uppercase()
        .find(" SET ")
        .ok_or_else(|| "UPDATE: missing SET".to_string())?;
    let table = after[..set_pos].trim().to_string();
    let rest = &after[set_pos + 5..];

    let (set_part, filter) = if let Some(wp) = rest.to_uppercase().find(" WHERE ") {
        (&rest[..wp], parse_where(rest[wp + 7..].trim()))
    } else {
        (rest, None)
    };
    let set = parse_where(set_part.trim())
        .ok_or_else(|| "UPDATE: invalid SET clause".to_string())?;

    if table.is_empty() {
        return Err("UPDATE: missing table".to_string());
    }
    Ok(Statement::Update { table, set, filter })
}

fn parse_drop(sql: &str) -> Result<Statement, String> {
    // DROP TABLE [IF EXISTS] t
    let after = sql[10..].trim(); // "DROP TABLE".len() == 10
    let after = after
        .strip_prefix("IF EXISTS")
        .or_else(|| after.strip_prefix("if exists"))
        .map(|s| s.trim())
        .unwrap_or(after);
    let table = after.trim().to_string();
    if table.is_empty() {
        return Err("DROP TABLE: missing table".to_string());
    }
    Ok(Statement::DropTable { table })
}

// ─────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_table() {
        let s = parse("CREATE TABLE users (id INT, name TEXT)").unwrap();
        assert_eq!(
            s,
            Statement::CreateTable {
                table: "users".into(),
                columns: vec![
                    ColumnDef { name: "id".into(), ty: ColumnType::Int },
                    ColumnDef { name: "name".into(), ty: ColumnType::Text },
                ],
            }
        );
    }

    #[test]
    fn test_create_table_if_not_exists() {
        let s = parse("CREATE TABLE IF NOT EXISTS t (a BIGINT, b TEXT)").unwrap();
        if let Statement::CreateTable { table, columns } = s {
            assert_eq!(table, "t");
            let names: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
            assert_eq!(names, vec!["a", "b"]);
            assert_eq!(columns[0].ty, ColumnType::BigInt);
        } else {
            panic!("expected CreateTable");
        }
    }

    #[test]
    fn test_insert() {
        let s = parse("INSERT INTO users (id, name) VALUES (1, 'Alice')").unwrap();
        assert_eq!(
            s,
            Statement::Insert {
                table: "users".into(),
                columns: vec!["id".into(), "name".into()],
                values: vec!["1".into(), "Alice".into()],
            }
        );
    }

    #[test]
    fn test_select_all() {
        let s = parse("SELECT * FROM users").unwrap();
        assert_eq!(
            s,
            Statement::Select {
                table: "users".into(),
                columns: None,
                filter: None,
            }
        );
    }

    #[test]
    fn test_select_where() {
        let s = parse("SELECT name FROM users WHERE id = '1'").unwrap();
        assert_eq!(
            s,
            Statement::Select {
                table: "users".into(),
                columns: Some(vec!["name".into()]),
                filter: Some(("id".into(), "1".into())),
            }
        );
    }

    #[test]
    fn test_aruaru_commit() {
        let s = parse("SELECT aruaru_commit('my message')").unwrap();
        assert_eq!(
            s,
            Statement::AruaruFn {
                name: "aruaru_commit".into(),
                arg: Some("my message".into()),
            }
        );
    }

    #[test]
    fn test_aruaru_log() {
        let s = parse("SELECT * FROM aruaru_log LIMIT 10").unwrap();
        assert_eq!(s, Statement::AruaruLog { limit: Some(10) });
    }

    #[test]
    fn test_delete() {
        let s = parse("DELETE FROM users WHERE id = '1'").unwrap();
        assert_eq!(
            s,
            Statement::Delete {
                table: "users".into(),
                filter: Some(("id".into(), "1".into())),
            }
        );
    }

    #[test]
    fn test_delete_all() {
        let s = parse("DELETE FROM users").unwrap();
        assert_eq!(s, Statement::Delete { table: "users".into(), filter: None });
    }

    #[test]
    fn test_update() {
        let s = parse("UPDATE users SET name = 'Carol' WHERE id = '2'").unwrap();
        assert_eq!(
            s,
            Statement::Update {
                table: "users".into(),
                set: ("name".into(), "Carol".into()),
                filter: Some(("id".into(), "2".into())),
            }
        );
    }

    #[test]
    fn test_drop_table() {
        assert_eq!(parse("DROP TABLE t").unwrap(), Statement::DropTable { table: "t".into() });
        assert_eq!(
            parse("DROP TABLE IF EXISTS t").unwrap(),
            Statement::DropTable { table: "t".into() }
        );
    }
}
