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

/// `ON CONFLICT ... DO ...` の挙動
#[derive(Debug, Clone, PartialEq)]
pub enum ConflictAction {
    /// DO NOTHING — 既存行があれば何もしない (冪等な「無ければ作る」)
    DoNothing,
    /// DO UPDATE SET col = value [, ...] — 既存行のみ指定列を更新
    DoUpdate(Vec<(String, ConflictValue)>),
}

/// DO UPDATE SET の右辺値。リテラルか、`EXCLUDED.col`(INSERTしようとした新しい値)か。
#[derive(Debug, Clone, PartialEq)]
pub enum ConflictValue {
    Literal(String),
    /// `EXCLUDED.col_name` — INSERT側の新しい値を使う (PostgreSQL互換)
    Excluded(String),
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
    /// INSERT ... ON CONFLICT (col) DO UPDATE SET ... / DO NOTHING
    ///
    /// open-runo 互換のUPSERT構文。`aruaru-db` はテーブルの先頭列を常にPKとして
    /// 扱うため、`conflict_column` はドキュメント目的の検証にのみ使い、実際の
    /// 衝突判定は行(row)の先頭列(PK)の重複で行う。
    Upsert {
        table: String,
        columns: Vec<String>,
        values: Vec<String>,
        /// ON CONFLICT (col) の col。省略時 (`ON CONFLICT DO ...`) は None。
        conflict_column: Option<String>,
        action: ConflictAction,
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
    /// `SELECT ... FROM table WHERE pk = 'v' AS OF COMMIT 'commit_id'`
    ///
    /// VersionLessAPI + Git 版管理ハイブリッドの読み出し側 (open-web-server/
    /// CLAUDE.md 拡張要件(1) の残ギャップ)。通常の `Select` と同じ
    /// `table`/`filter` に加え、参照する過去コミットのIDを持つ。
    /// 現状は単一行 (PK 一致) の読み出しのみサポート (フルスキャン AS OF は
    /// 将来の拡張、下記 engine.rs のドキュコメント参照)。
    SelectAsOf {
        table: String,
        filter: Option<(String, String)>,
        commit_id: String,
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
    // INSERT INTO name (c1, c2) VALUES (v1, v2) [ON CONFLICT (c) DO UPDATE SET .../DO NOTHING]
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
        .find(')')
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

    // ── ON CONFLICT (省略可) ─────────────────────────────────
    let remainder = values_part[vclose + 1..].trim();
    if remainder.to_uppercase().starts_with("ON CONFLICT") {
        let (conflict_column, action) = parse_on_conflict(remainder)?;
        return Ok(Statement::Upsert {
            table,
            columns,
            values,
            conflict_column,
            action,
        });
    }

    Ok(Statement::Insert {
        table,
        columns,
        values,
    })
}

/// `ON CONFLICT [(col)] DO NOTHING` / `ON CONFLICT [(col)] DO UPDATE SET c1 = v1 [, c2 = v2 ...]`
fn parse_on_conflict(sql: &str) -> Result<(Option<String>, ConflictAction), String> {
    // "ON CONFLICT".len() == 11
    let after = sql[11..].trim();

    let (conflict_column, after_target) = if let Some(rest) = after.strip_prefix('(') {
        let close = rest
            .find(')')
            .ok_or_else(|| "ON CONFLICT: missing ')' in conflict target".to_string())?;
        let col = rest[..close].trim().to_string();
        (Some(col), rest[close + 1..].trim())
    } else {
        (None, after)
    };

    let upper = after_target.to_uppercase();
    if upper.starts_with("DO NOTHING") {
        return Ok((conflict_column, ConflictAction::DoNothing));
    }

    if !upper.starts_with("DO UPDATE SET") {
        return Err(format!(
            "ON CONFLICT: expected DO NOTHING or DO UPDATE SET, got: {}",
            after_target
        ));
    }
    // "DO UPDATE SET".len() == 13
    let set_list = after_target[13..].trim();
    let assignments: Vec<(String, ConflictValue)> = set_list
        .split(',')
        .map(|clause| {
            let eq = clause
                .find('=')
                .ok_or_else(|| format!("ON CONFLICT DO UPDATE: invalid assignment: {}", clause))?;
            let col = clause[..eq].trim().to_string();
            let raw_val = clause[eq + 1..].trim();
            let val = if let Some(excl_col) = raw_val
                .to_uppercase()
                .strip_prefix("EXCLUDED.")
                .map(|_| raw_val[9..].trim().to_string())
            {
                ConflictValue::Excluded(excl_col)
            } else {
                ConflictValue::Literal(
                    raw_val.trim_matches(|c| c == '\'' || c == '"').to_string(),
                )
            };
            Ok::<_, String>((col, val))
        })
        .collect::<Result<Vec<_>, String>>()?;

    if assignments.is_empty() {
        return Err("ON CONFLICT DO UPDATE SET: empty assignment list".to_string());
    }

    Ok((conflict_column, ConflictAction::DoUpdate(assignments)))
}

fn parse_select(sql: &str) -> Result<Statement, String> {
    // SELECT cols FROM table [WHERE col = 'val'] [AS OF COMMIT 'commit_id']
    let upper_full = sql.to_uppercase();
    if let Some(as_of_pos) = upper_full.find(" AS OF COMMIT ") {
        let head = sql[..as_of_pos].trim();
        let commit_part = sql[as_of_pos + " AS OF COMMIT ".len()..].trim();
        let commit_id = commit_part
            .trim_matches(|c| c == '\'' || c == '"')
            .to_string();
        if commit_id.is_empty() {
            return Err("AS OF COMMIT requires a commit id".to_string());
        }
        // 残りは普通の SELECT として再パースし、table/filterを流用する。
        let inner = match parse_select(head)? {
            Statement::Select { table, filter, .. } => (table, filter),
            other => return Err(format!("AS OF COMMIT: unsupported inner statement {other:?}")),
        };
        return Ok(Statement::SelectAsOf {
            table: inner.0,
            filter: inner.1,
            commit_id,
        });
    }

    let upper = upper_full;
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

    #[test]
    fn test_insert_on_conflict_do_nothing() {
        let s = parse(
            "INSERT INTO users (id, name) VALUES (1, 'Alice') ON CONFLICT (id) DO NOTHING",
        )
        .unwrap();
        assert_eq!(
            s,
            Statement::Upsert {
                table: "users".into(),
                columns: vec!["id".into(), "name".into()],
                values: vec!["1".into(), "Alice".into()],
                conflict_column: Some("id".into()),
                action: ConflictAction::DoNothing,
            }
        );
    }

    #[test]
    fn test_insert_on_conflict_do_update() {
        let s = parse(
            "INSERT INTO wallets (id, balance) VALUES (1, '100') ON CONFLICT (id) DO UPDATE SET balance = EXCLUDED.balance",
        )
        .unwrap();
        assert_eq!(
            s,
            Statement::Upsert {
                table: "wallets".into(),
                columns: vec!["id".into(), "balance".into()],
                values: vec!["1".into(), "100".into()],
                conflict_column: Some("id".into()),
                action: ConflictAction::DoUpdate(vec![(
                    "balance".into(),
                    ConflictValue::Excluded("balance".into())
                )]),
            }
        );
    }

    #[test]
    fn test_insert_on_conflict_do_update_literal_and_multi() {
        let s = parse(
            "INSERT INTO items (id, qty) VALUES (1, '5') ON CONFLICT (id) DO UPDATE SET qty = '5', status = 'granted'",
        )
        .unwrap();
        assert_eq!(
            s,
            Statement::Upsert {
                table: "items".into(),
                columns: vec!["id".into(), "qty".into()],
                values: vec!["1".into(), "5".into()],
                conflict_column: Some("id".into()),
                action: ConflictAction::DoUpdate(vec![
                    ("qty".into(), ConflictValue::Literal("5".into())),
                    ("status".into(), ConflictValue::Literal("granted".into())),
                ]),
            }
        );
    }

    #[test]
    fn test_insert_on_conflict_no_target_column() {
        // ON CONFLICT DO NOTHING (衝突対象列を明示しない open-runo 生成SQLにも対応)
        let s = parse("INSERT INTO users (id, name) VALUES (1, 'Bob') ON CONFLICT DO NOTHING")
            .unwrap();
        assert_eq!(
            s,
            Statement::Upsert {
                table: "users".into(),
                columns: vec!["id".into(), "name".into()],
                values: vec!["1".into(), "Bob".into()],
                conflict_column: None,
                action: ConflictAction::DoNothing,
            }
        );
    }

    #[test]
    fn test_insert_plain_still_works_without_on_conflict() {
        // 既存の非UPSERT INSERTが壊れていないことの回帰確認
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
}
