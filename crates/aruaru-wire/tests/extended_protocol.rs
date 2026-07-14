//! Real end-to-end proof that the extended (prepared statement) pgwire
//! protocol works against this server -- not just the Simple Query
//! protocol `psql` uses.
//!
//! Before 2026-07-14, `AruaruHandler::do_describe_portal` always returned
//! an empty column list (the server's schema is dynamic, so there was no
//! way to know the columns without actually running the query). Clients
//! using the extended protocol -- which is most ORMs/drivers' default,
//! `sqlx`'s `query_as`/`.bind()` among them -- decode `Execute`'s row data
//! using the column shape they got from `Describe`, so any `SELECT`
//! returning rows failed with `ColumnIndexOutOfBounds`. `psql` itself uses
//! the simple query protocol and was never affected, which is why this
//! went unnoticed for a while.
//!
//! Spawns the actual `aruaru-server` binary as a child process and
//! connects with `sqlx::query_as`/`.bind()` (the extended protocol path),
//! matching what `crates/open-runo-db/tests/aruaru_as_of_commit.rs` (in
//! the sibling `open-runo`/`poem-cosmo-tauri` repos) had to work around
//! with `sqlx::raw_sql` before this fix.

use sqlx::PgPool;
use std::process::{Child, Command};
use std::time::Duration;

struct ServerGuard(Child);
impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn spawn_server_and_connect(port: u16) -> (ServerGuard, PgPool) {
    let binary = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target")
        .join("debug")
        .join(if cfg!(windows) { "aruaru-server.exe" } else { "aruaru-server" });
    assert!(
        binary.exists(),
        "aruaru-server binary not found at {binary:?} -- build it first: `cargo build -p aruaru-server`"
    );

    let data_dir = std::env::temp_dir().join(format!("aruaru-extended-protocol-test-{port}-{}", std::process::id()));
    std::fs::create_dir_all(&data_dir).expect("create temp data dir");

    let child = Command::new(&binary)
        .arg("--pg-port").arg(port.to_string())
        .arg("--gql-port").arg("0")
        .arg("--data").arg(&data_dir)
        .arg("--log-level").arg("warn")
        .env("ARUARU_USERS", "aruaru:aruaru")
        .spawn()
        .expect("spawn aruaru-server");
    let guard = ServerGuard(child);

    let url = format!("postgres://aruaru:aruaru@127.0.0.1:{port}/aruaru");
    let mut last_err = None;
    for _ in 0..50 {
        match PgPool::connect(&url).await {
            Ok(pool) => return (guard, pool),
            Err(e) => {
                last_err = Some(e);
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
    panic!("could not connect to aruaru-server pgwire endpoint: {last_err:?}");
}

#[tokio::test]
#[ignore = "spawns the real aruaru-server binary; run explicitly with --ignored"]
async fn extended_protocol_select_returns_real_rows_not_column_index_out_of_bounds() {
    let (_guard, pool) = spawn_server_and_connect(15434).await;

    // Simple Query (raw_sql) to create the table and insert a row, so the
    // extended-protocol SELECT below has real data to decode -- this half
    // was never broken (psql uses the same protocol), included only for
    // setup, not as part of what's under test.
    sqlx::raw_sql("CREATE TABLE IF NOT EXISTS widgets (pk TEXT, name TEXT, qty TEXT)")
        .execute(&pool)
        .await
        .expect("create table");
    sqlx::raw_sql("INSERT INTO widgets (pk, name, qty) VALUES ('w1', 'sprocket', '7')")
        .execute(&pool)
        .await
        .expect("insert row");

    // The actual regression case: `query_as` + `.bind()` is sqlx's
    // extended-protocol path (Parse -> Describe -> Bind -> Execute).
    // Before the fix, `Describe` returned zero columns and decoding the
    // 3-column row from `Execute` panicked with `ColumnIndexOutOfBounds`.
    let row: (String, String, String) = sqlx::query_as("SELECT pk, name, qty FROM widgets WHERE pk = $1")
        .bind("w1")
        .fetch_one(&pool)
        .await
        .expect("extended-protocol SELECT should decode real columns, not fail with ColumnIndexOutOfBounds");

    assert_eq!(row, ("w1".to_string(), "sprocket".to_string(), "7".to_string()));
}

#[tokio::test]
#[ignore = "spawns the real aruaru-server binary; run explicitly with --ignored"]
async fn extended_protocol_never_double_executes_a_git_on_sql_mutation() {
    let (_guard, pool) = spawn_server_and_connect(15435).await;

    // `SELECT aruaru_commit('...')` is syntactically a SELECT but is a
    // Git-on-SQL mutation (it advances the commit log). The fix must
    // classify this as non-read-only (via aruaru_query::parser::Statement,
    // not a raw "starts with SELECT" check) and NOT pre-execute it during
    // Describe -- otherwise every commit made over the extended protocol
    // would silently happen twice.
    sqlx::raw_sql("CREATE TABLE IF NOT EXISTS ledger (pk TEXT, value TEXT)")
        .execute(&pool)
        .await
        .expect("create table");
    sqlx::raw_sql("INSERT INTO ledger (pk, value) VALUES ('k1', 'v1')")
        .execute(&pool)
        .await
        .expect("insert row");

    // The engine's SQL subset has no aggregate functions (no COUNT(*)),
    // so count log entries by fetching all rows -- `LIMIT 1000` keeps
    // this well above any realistic commit count from this test alone.
    let before: Vec<(String, String, String, String)> = sqlx::query_as("SELECT * FROM aruaru_log LIMIT 1000")
        .fetch_all(&pool)
        .await
        .expect("list commit log entries before");

    // Extended protocol, matching how a real driver would issue this.
    let _commit_id: (String,) = sqlx::query_as("SELECT aruaru_commit($1)")
        .bind("extended protocol test commit")
        .fetch_one(&pool)
        .await
        .expect("aruaru_commit over the extended protocol should succeed");

    let after: Vec<(String, String, String, String)> = sqlx::query_as("SELECT * FROM aruaru_log LIMIT 1000")
        .fetch_all(&pool)
        .await
        .expect("list commit log entries after");

    assert_eq!(
        after.len(),
        before.len() + 1,
        "exactly one commit-log entry should be added, not two (which would mean the Describe \
         phase double-executed the mutation)"
    );
}
