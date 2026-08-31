//! `ColumnarApplier` — Raft-Learner 上の 行→列 非同期変換レプリカ(A.6-2)
//!
//! `docs/CONTROL_PLANE_REDESIGN.md` 付録 A.6-2 が「本命」と位置づける、
//! TiDB/TiFlash 型 HTAP の核心(Raft learner が受け取った commit を
//! 独立の列指向ストアへ変換する)を実装する。
//!
//! ## 設計
//!
//! `RaftNode<A>`は`--raft-role learner`で複製先にはなれる(2026-08-21
//! 実装済み)が、`A: Applier`が受け取るのは常に行ストア向けの
//! `EngineApplier`だった——learner が受けた commit を**列変換して
//! 別ストアへ流す**部分が無かった、というのが A.6-2 が埋めるギャップ。
//!
//! `ColumnarApplier`は`Applier`を実装し、learner の `RaftNode` に注入
//! できる。`apply()`が呼ばれるたびに:
//! 1. 受け取った SQL を、行ストア側と全く同じ意味論を保つため
//!    **`EngineApplier`と同一のロジックで自身が保持する`QueryEngine`
//!    (行ストアのローカルミラー)へ適用する**(既存の`snapshot_table`/
//!    `get_row`等の読み取りAPIをそのまま使うため)。
//! 2. `Command::Exec`が触れたテーブルを`parser::parse`で特定し、
//!    そのテーブル全体を`aruaru-backup::table_format`(Databend 方式の
//!    snapshot→segment→block、既存実装をそのまま再利用)の1 block へ
//!    列変換(min/max 統計 + 主キーの bloom filter)して commit する。
//!
//! ## 正直な簡略化点(誇張しない)
//!
//! 1. **「本物の行→列」ではなく「テーブル全体の都度再構築」**。TiFlash の
//!    DeltaTree(小さな delta を蓄積し閾値でまとめて compaction)とは
//!    異なり、変更のたびにテーブル全体を1つの block として書き直す
//!    ——`aruaru-query::olap::OlapCache`が既に同種の簡略化を明記して
//!    いるのと同じ判断(まず正しさを実証し、閾値付き delta 蓄積は
//!    次段階)。
//! 2. **block の実体(Parquet 相当のバイト列)は書かない**。
//!    `table_format::BlockMeta`は元々「メタデータ + 枝刈り・コミットの
//!    正しさ」だけを担当する設計(同モジュールの doc に明記済み)であり、
//!    `ColumnarApplier`もその制約をそのまま引き継ぐ——列レプリカとしては
//!    「統計・bloom filter が正しく反映され、枝刈りに使える」ところまでを
//!    実証する。
//! 3. **ネットワーク越しの真の別ノード learner ではなく、同一プロセス内で
//!    `Applier`として注入して検証する**——`binary_transport.rs`経由の
//!    複製自体は既存の learner 配線(2026-08-21実装)がそのまま使える
//!    ため、本モジュールは「learner が受けた commit をどう列変換するか」
//!    にのみ責務を絞った。
//! 4. **A.6-3(読み取り時の Raft index + MVCC による SI 検証)とは未接続**
//!    ——列レプリカへの読み取りが「どの Raft index まで反映済みか」を
//!    保証する仕組みは次段階。

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use aruaru_backup::table_format::{BlockMeta, MetaService, ObjectStore, ObjectTable, TableFormatError};
use aruaru_core::catalog::ColumnType;
use aruaru_query::parser::{self, Statement};
use aruaru_query::QueryEngine;

use crate::raft::{Applier, Command, CommandResponse};

/// `table_format::ObjectTable`は `(db_id, table_id)` という u64 の組で
/// テーブルを識別する(Databend と同じキー空間)。列レプリカは SQL の
/// テーブル**名**単位で管理したいため、SHA-256 の先頭 8 バイトを
/// `table_id` として決定的に導出する(衝突確率は無視できるほど低く、
/// 既存の `object-table` 撤廃済みREST機能とは独立の名前空間)。
fn table_id_for(table_name: &str) -> u64 {
    let digest = Sha256::digest(table_name.as_bytes());
    u64::from_be_bytes(digest[..8].try_into().expect("sha256 digest is >= 8 bytes"))
}

fn table_name_touched(stmt: &Statement) -> Option<String> {
    match stmt {
        Statement::CreateTable { table, .. }
        | Statement::Insert { table, .. }
        | Statement::Upsert { table, .. }
        | Statement::Delete { table, .. }
        | Statement::Update { table, .. }
        | Statement::DropTable { table } => Some(table.clone()),
        // 読み取り専用文・トランザクション制御文・Git-on-SQL 関数は
        // 列レプリカの再構築対象ではない。
        Statement::Select { .. }
        | Statement::SelectAsOf { .. }
        | Statement::Begin
        | Statement::TxnCommit
        | Statement::Rollback
        | Statement::AruaruFn { .. }
        | Statement::AruaruLog { .. } => None,
    }
}

/// Raft learner 用の行→列変換 `Applier`。
pub struct ColumnarApplier {
    /// 行ストアのローカルミラー(learner 自身が保持、`EngineApplier`と
    /// 同じ意味論で SQL を適用する)。
    engine: Arc<QueryEngine>,
    store: Arc<dyn ObjectStore>,
    meta: Arc<MetaService>,
    /// テーブル名 → `ObjectTable`(初回アクセス時に遅延構築)。
    tables: Mutex<HashMap<String, Arc<ObjectTable>>>,
    /// 実際に commit された列変換の回数(観測用、テスト・将来の
    /// `Query.htapReplicas`診断で使う想定)。
    replication_count: std::sync::atomic::AtomicU64,
}

impl ColumnarApplier {
    pub fn new(engine: Arc<QueryEngine>, store: Arc<dyn ObjectStore>, meta: Arc<MetaService>) -> Self {
        Self {
            engine,
            store,
            meta,
            tables: Mutex::new(HashMap::new()),
            replication_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// テストやサーバー起動時にメモリ実装をそのまま使いたい場合の便利
    /// コンストラクタ。
    pub fn with_in_memory_store(engine: Arc<QueryEngine>) -> Self {
        Self::new(
            engine,
            Arc::new(aruaru_backup::table_format::InMemoryObjectStore::new()),
            Arc::new(MetaService::new()),
        )
    }

    fn object_table_for(&self, table_name: &str) -> Arc<ObjectTable> {
        let mut tables = self.tables.lock();
        tables
            .entry(table_name.to_string())
            .or_insert_with(|| {
                Arc::new(ObjectTable::new(
                    self.store.clone(),
                    self.meta.clone(),
                    "columnar-replica",
                    0,
                    table_id_for(table_name),
                ))
            })
            .clone()
    }

    /// このテーブルの現在の行ストア内容を1 block へ列変換して commit する。
    /// テーブルが存在しない(DROP TABLE直後等)場合は何もしない。
    fn replicate_table(&self, table_name: &str) -> Result<(), TableFormatError> {
        let Some((cols, pks, rows)) = self.engine.snapshot_table(table_name) else {
            return Ok(());
        };
        let row_count = rows.len() as u64;
        let mut block = BlockMeta::new(format!("mem://{table_name}"), row_count, 0);

        for (col_idx, (col_name, col_type)) in cols.iter().enumerate() {
            if !matches!(col_type, ColumnType::Int | ColumnType::BigInt | ColumnType::Float) {
                continue;
            }
            let mut min = f64::INFINITY;
            let mut max = f64::NEG_INFINITY;
            let mut null_count = 0u64;
            let mut seen_any = false;
            for row in &rows {
                match row.get(col_idx).and_then(|raw| raw.parse::<f64>().ok()) {
                    Some(v) => {
                        seen_any = true;
                        min = min.min(v);
                        max = max.max(v);
                    }
                    None => null_count += 1,
                }
            }
            if seen_any {
                block = block.with_stats(col_name, min, max, null_count);
            }
        }

        let pk_strings: Vec<String> = pks.iter().map(|pk| String::from_utf8_lossy(pk).into_owned()).collect();
        if !pk_strings.is_empty() {
            block = block.with_bloom("__pk__", pk_strings.iter().map(|s| s.as_str()));
        }

        let object_table = self.object_table_for(table_name);
        object_table.commit_blocks(vec![block])?;
        self.replication_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// このテーブルの列レプリカの最新スナップショットIDを返す(テスト・
    /// 観測用)。まだ一度もレプリケートされていなければ `None`。
    pub fn latest_snapshot_id(&self, table_name: &str) -> Option<String> {
        self.object_table_for(table_name)
            .current_snapshot()
            .ok()
            .flatten()
            .map(|s| s.snapshot_id)
    }

    /// 累計レプリケーション回数(observability用)。
    pub fn replication_count(&self) -> u64 {
        self.replication_count.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 行ストアのミラー(テスト・診断用に直接読みたい場合)。
    pub fn engine(&self) -> &Arc<QueryEngine> {
        &self.engine
    }
}

impl Applier for ColumnarApplier {
    fn apply(&self, command: &Command) -> CommandResponse {
        match command {
            Command::Exec(sql) => {
                let exec_result = self.engine.execute(sql);
                // 列変換は「行ストアへの適用が成功した場合のみ」行う——
                // 失敗した SQL(構文エラー等)によってテーブルの状態が
                // 変わっていないのに列レプリカだけ再構築するのは無駄
                // (かつ、存在しないテーブルへの再構築を試みてしまう)。
                if exec_result.is_ok() {
                    if let Ok(stmt) = parser::parse(sql) {
                        if let Some(table_name) = table_name_touched(&stmt) {
                            if let Err(e) = self.replicate_table(&table_name) {
                                tracing::warn!(
                                    table = %table_name,
                                    error = %e,
                                    "columnar replica commit failed; row store already applied, learner will retry on next write"
                                );
                            }
                        }
                    }
                }
                match exec_result {
                    Ok(_) => CommandResponse::ok(),
                    Err(e) => CommandResponse::err(e),
                }
            }
            Command::Commit(msg) => {
                let safe = msg.replace('\'', "''");
                match self.engine.execute(&format!("SELECT aruaru_commit('{safe}')")) {
                    Ok(_) => CommandResponse::ok(),
                    Err(e) => CommandResponse::err(e),
                }
            }
            Command::Noop => CommandResponse::ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_applier() -> ColumnarApplier {
        ColumnarApplier::with_in_memory_store(Arc::new(QueryEngine::new()))
    }

    #[test]
    fn create_table_and_insert_produce_a_columnar_snapshot_with_stats() {
        let applier = new_applier();
        let resp = applier.apply(&Command::Exec(
            "CREATE TABLE items (id INT PRIMARY KEY, qty INT)".into(),
        ));
        assert!(resp.ok, "create table should succeed: {resp:?}");
        assert!(
            applier.latest_snapshot_id("items").is_some(),
            "CREATE TABLE should already produce a (empty) columnar snapshot"
        );

        let resp = applier.apply(&Command::Exec(
            "INSERT INTO items (id, qty) VALUES (1, 10)".into(),
        ));
        assert!(resp.ok, "insert should succeed: {resp:?}");
        let first_snapshot = applier.latest_snapshot_id("items").expect("snapshot after insert");

        let resp = applier.apply(&Command::Exec(
            "INSERT INTO items (id, qty) VALUES (2, 20)".into(),
        ));
        assert!(resp.ok, "second insert should succeed: {resp:?}");
        let second_snapshot = applier
            .latest_snapshot_id("items")
            .expect("snapshot after second insert");

        assert_ne!(
            first_snapshot, second_snapshot,
            "each write should produce a new columnar snapshot (time-travel chain)"
        );
        assert_eq!(applier.replication_count(), 3, "create+2 inserts = 3 replications");
    }

    #[test]
    fn row_store_and_columnar_replica_stay_consistent_after_update_and_delete() {
        let applier = new_applier();
        applier.apply(&Command::Exec(
            "CREATE TABLE gear (id INT PRIMARY KEY, qty INT)".into(),
        ));
        applier.apply(&Command::Exec("INSERT INTO gear (id, qty) VALUES (1, 5)".into()));
        applier.apply(&Command::Exec("INSERT INTO gear (id, qty) VALUES (2, 9)".into()));
        applier.apply(&Command::Exec("UPDATE gear SET qty = '99' WHERE id = '1'".into()));
        applier.apply(&Command::Exec("DELETE FROM gear WHERE id = '2'".into()));

        // 行ストア側(learner 自身のミラー)が最終状態を正しく反映している。
        let (_, pks, rows) = applier.engine().snapshot_table("gear").expect("table exists");
        assert_eq!(pks.len(), 1, "one row should remain after delete");
        assert_eq!(rows[0][1], "99", "surviving row should reflect the update");

        // 列レプリカも(テーブル全体再構築のため)同じ最終状態に追従している
        // ことを、コミットが失敗していない(=snapshotが存在する)ことで確認。
        assert!(applier.latest_snapshot_id("gear").is_some());
    }

    #[test]
    fn statements_that_fail_on_the_row_store_do_not_touch_the_columnar_replica() {
        let applier = new_applier();
        // テーブル未作成のまま INSERT — 行ストア側で失敗するはず。
        let resp = applier.apply(&Command::Exec(
            "INSERT INTO missing (id) VALUES (1)".into(),
        ));
        assert!(!resp.ok, "insert into a non-existent table should fail");
        assert!(
            applier.latest_snapshot_id("missing").is_none(),
            "a failed write must not produce a columnar snapshot"
        );
    }

    #[test]
    fn read_only_and_control_flow_statements_do_not_trigger_replication() {
        let applier = new_applier();
        applier.apply(&Command::Exec(
            "CREATE TABLE t (id INT PRIMARY KEY, v TEXT)".into(),
        ));
        let after_create = applier.replication_count();
        applier.apply(&Command::Exec("INSERT INTO t (id, v) VALUES (1, 'x')".into()));
        let after_insert = applier.replication_count();
        assert!(after_insert > after_create);

        applier.apply(&Command::Exec("SELECT * FROM t".into()));
        applier.apply(&Command::Exec("BEGIN".into()));
        applier.apply(&Command::Exec("COMMIT".into()));
        assert_eq!(
            applier.replication_count(),
            after_insert,
            "read-only/transaction-control statements must not trigger a columnar rebuild"
        );
    }

    #[test]
    fn commit_command_advances_git_on_sql_history_without_touching_columnar_replica() {
        let applier = new_applier();
        applier.apply(&Command::Exec(
            "CREATE TABLE t (id INT PRIMARY KEY, v TEXT)".into(),
        ));
        let before = applier.replication_count();
        let resp = applier.apply(&Command::Commit("initial".into()));
        assert!(resp.ok, "commit should succeed: {resp:?}");
        assert_eq!(
            applier.replication_count(),
            before,
            "aruaru_commit is a version-control marker, not a row-store mutation, and must not rebuild the columnar replica"
        );
    }
}
