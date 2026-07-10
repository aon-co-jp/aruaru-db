//! aruaru-query: クエリ実行エンジン
//!
//! v0.3 段階では、pgwire を駆動するために必要な **SQL サブセット** を実装する:
//! - `CREATE TABLE t (col1, col2, ...)`
//! - `INSERT INTO t (cols) VALUES (...)`
//! - `SELECT * FROM t` / `SELECT ... FROM t WHERE pk = '...'`
//! - Git-on-SQL 関数: `aruaru_branch / aruaru_checkout / aruaru_commit / aruaru_merge`
//! - システムテーブル: `aruaru_log`
//!
//! 完全な SQL パーサ (sqlparser) への置き換えは v0.4 で行う。
//! ここではコンパイル安定性と正しさを優先した手書きサブセットパーサを使う。

pub mod engine;
pub mod olap;
pub mod parser;

pub use engine::{QueryEngine, QueryResponse, Value};

/// クエリの種別 (HTAP ルーターが決定)
#[derive(Debug, Clone, PartialEq)]
pub enum QueryKind {
    Oltp, // 点検索・短トランザクション
    Olap, // 集計・JOIN・GROUP BY
}

/// HTAP ルーター: SQL を解析して OLTP / OLAP を判定
pub fn classify_query(sql: &str) -> QueryKind {
    let upper = sql.to_uppercase();
    if upper.contains("GROUP BY")
        || upper.contains("SUM(")
        || upper.contains("COUNT(")
        || upper.contains("AVG(")
        || upper.contains("WINDOW")
    {
        QueryKind::Olap
    } else {
        QueryKind::Oltp
    }
}
