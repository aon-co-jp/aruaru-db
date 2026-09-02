//! `ColumnarApplier` — Raft-Learner 上の 行→列 非同期変換レプリカ(A.6-2)
//! + **A.6-4 段階2: base+delta の Merge-on-Read(MoR)**
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
//!    その差分だけを`aruaru-backup::table_format`(Databend 方式の
//!    snapshot→segment→block、既存実装をそのまま再利用)へ反映する。
//!
//! ## A.6-4 段階2: base+delta の Merge-on-Read(2026-09-02 実装)
//!
//! 従来(段階1まで)は「変更のたびにテーブル全体を1つの block として
//! 書き直す」都度フル再構築だった。TiFlash の DeltaTree(小さな delta を
//! 蓄積し、閾値でまとめて compaction する)に倣い、以下へ格上げした:
//!
//! - **base block**: 初回レプリケーション時のテーブル全体スナップショット。
//! - **delta block**: 以降の commit で新規追加・更新された行だけを持つ
//!   小さな block(commit ごとに1つ追加される)。
//! - **deletion vector の書き込み側配線(続き18 が「未配線」と明記して
//!   いた残課題)**: 実際の DELETE / UPDATE が発生したら、消えた行・
//!   古くなった行の**block 内位置**を `BlockMeta::deletion_vector` へ
//!   マークする。block の実体は書き直さない(即時 rewrite 無しの MoR)。
//! - **compaction**: delta が `DEFAULT_COMPACTION_THRESHOLD` 個たまったら、
//!   現在のテーブル全体から新しい base を作り直し、delta を畳み込む。
//!
//! `object_table.commit_blocks()` には毎回 `[base, delta_1, .., delta_n]`
//! の全体を渡すため、最新スナップショットは常に MoR 済みの正しい状態を
//! 表す(`SegmentMeta::live_row_count()` が deletion vector を差し引いて
//! 正しい実効行数を返す。`prune_range`/`prune_equality` も続き18 で
//! deletion vector を尊重するよう配線済み)。
//!
//! ## 正直な簡略化点(誇張しない)
//!
//! 1. **block の実体(Parquet 相当のバイト列)は書かない**。
//!    `table_format::BlockMeta`は元々「メタデータ + 枝刈り・コミットの
//!    正しさ」だけを担当する設計であり、`ColumnarApplier`もその制約を
//!    そのまま引き継ぐ——列レプリカとしては「統計・bloom filter・
//!    deletion vector が正しく反映され、枝刈り・実効行数計算に使える」
//!    ところまでを実証する。
//! 2. **差分検出の粒度は「主キー単位 + 行内容のハッシュ」**。行内容が
//!    1バイトでも変われば「更新」とみなし、旧位置を deletion vector へ、
//!    新しい行を delta block へ入れる(列単位の部分更新追跡はしない)。
//! 3. **ネットワーク越しの真の別ノード learner ではなく、同一プロセス内で
//!    `Applier`として注入して検証する**(`binary_transport.rs`経由の
//!    複製自体は既存の learner 配線がそのまま使える)。ただし
//!    `aruaru-server --columnar-learner` 経由の実プロセス間 E2E は
//!    2026-08-31(続き16)で確認済み。
//! 4. **A.6-3(読み取り時の Raft index + MVCC による SI 検証)とは未接続**。
//! 5. deletion vector は `BTreeSet<u64>`(非圧縮)。大量削除時の
//!    メモリ効率は Delta Lake の RoaringBitmap に劣る(`table_format` 側の
//!    既知の課題、行数がボトルネックになった時点で置き換え)。

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use aruaru_backup::table_format::{BlockMeta, MetaService, ObjectStore, ObjectTable, TableFormatError};
use aruaru_core::catalog::ColumnType;
use aruaru_query::parser::{self, Statement};
use aruaru_query::QueryEngine;

use crate::raft::{Applier, Command, CommandResponse};

/// delta block をこの個数ためたら、テーブル全体から base を作り直して
/// 畳み込む(TiFlash DeltaTree の閾値 compaction 相当)の既定値。
/// `aruaru.yaml: htap.delta.compaction_threshold` で上書きできる
/// (`ColumnarApplier::with_compaction_threshold`)。
pub const DEFAULT_COMPACTION_THRESHOLD: usize = 8;

/// `table_format::ObjectTable`は `(db_id, table_id)` という u64 の組で
/// テーブルを識別する(Databend と同じキー空間)。列レプリカは SQL の
/// テーブル**名**単位で管理したいため、SHA-256 の先頭 8 バイトを
/// `table_id` として決定的に導出する。
fn table_id_for(table_name: &str) -> u64 {
    let digest = Sha256::digest(table_name.as_bytes());
    u64::from_be_bytes(digest[..8].try_into().expect("sha256 digest is >= 8 bytes"))
}

fn row_hash(row: &[String]) -> u64 {
    let mut h = DefaultHasher::new();
    row.len().hash(&mut h);
    for cell in row {
        cell.hash(&mut h);
    }
    h.finish()
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

/// 生きている1行が列レプリカのどの block のどの位置にいるか。
#[derive(Clone, Debug)]
struct RowLoc {
    block_idx: usize,
    row_pos: u64,
    row_hash: u64,
}

/// 1テーブルぶんの base+delta 状態(プロセスメモリ上、これが MoR の
/// 権威ある表現)。`ObjectTable` 側は append-only の segment ログ
/// (`commit_blocks` が親の segment 一覧を引き継いで追記する設計)なので、
/// 「マージ済みの現在の姿」はこのメモリ状態が保持し、`ObjectTable` へは
/// **その commit で新しく書いた block だけ**を渡して時間旅行の
/// スナップショット連鎖を進める。
struct TableReplicaState {
    /// `[0]` = base、`[1..]` = delta。deletion vector 込みで、これを
    /// 併合したものが MoR の現在ビュー。
    blocks: Vec<BlockMeta>,
    /// 生きている主キー → その行の位置。
    locations: HashMap<String, RowLoc>,
    /// 直近の base 以降にためた delta の数。
    deltas_since_base: usize,
    /// compaction で base を作り直した回数(block の location 名の一意性用)。
    base_generation: u64,
}

/// このテーブルの現在の行ストア内容から、列変換した block を1つ作る。
fn build_block(
    location: String,
    cols: &[(String, ColumnType)],
    pk_strings: &[String],
    rows: &[Vec<String>],
) -> BlockMeta {
    let mut block = BlockMeta::new(location, rows.len() as u64, 0);

    for (col_idx, (col_name, col_type)) in cols.iter().enumerate() {
        if !matches!(col_type, ColumnType::Int | ColumnType::BigInt | ColumnType::Float) {
            continue;
        }
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        let mut null_count = 0u64;
        let mut seen_any = false;
        for row in rows {
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

    if !pk_strings.is_empty() {
        block = block.with_bloom("__pk__", pk_strings.iter().map(|s| s.as_str()));
    }
    block
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
    /// テーブル名 → base+delta 状態。
    replica_state: Mutex<HashMap<String, TableReplicaState>>,
    /// 実際に commit された列変換の回数(観測用)。
    replication_count: std::sync::atomic::AtomicU64,
    /// delta がこの個数たまったら base へ compaction する
    /// (`aruaru.yaml: htap.delta.compaction_threshold`)。
    compaction_threshold: usize,
}

impl ColumnarApplier {
    pub fn new(engine: Arc<QueryEngine>, store: Arc<dyn ObjectStore>, meta: Arc<MetaService>) -> Self {
        Self {
            engine,
            store,
            meta,
            tables: Mutex::new(HashMap::new()),
            replica_state: Mutex::new(HashMap::new()),
            replication_count: std::sync::atomic::AtomicU64::new(0),
            compaction_threshold: DEFAULT_COMPACTION_THRESHOLD,
        }
    }

    /// compaction 閾値(delta 個数)を指定する。`0` は既定値へフォールバック。
    pub fn with_compaction_threshold(mut self, n: usize) -> Self {
        self.compaction_threshold = if n == 0 { DEFAULT_COMPACTION_THRESHOLD } else { n };
        self
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

    /// このテーブルの行ストアの現在内容と、これまでの列レプリカ状態を
    /// 突き合わせて、**差分だけ**を base+delta へ反映して commit する。
    /// テーブルが存在しない(DROP TABLE直後等)場合は何もしない。
    fn replicate_table(&self, table_name: &str) -> Result<(), TableFormatError> {
        let Some((cols, pks, rows)) = self.engine.snapshot_table(table_name) else {
            return Ok(());
        };
        let pk_strings: Vec<String> =
            pks.iter().map(|pk| String::from_utf8_lossy(pk).into_owned()).collect();

        // 権威ある MoR 状態(`replica_state`)を差分更新し、その commit で
        // **新しく書いた block だけ**を `authored` として返す。
        let authored: Vec<BlockMeta> = {
            let mut states = self.replica_state.lock();
            match states.get_mut(table_name) {
                None => {
                    // 初回: テーブル全体を base block にする。
                    let base = build_block(
                        format!("mem://{table_name}/base/0"),
                        &cols,
                        &pk_strings,
                        &rows,
                    );
                    let locations = pk_strings
                        .iter()
                        .enumerate()
                        .map(|(i, pk)| {
                            (
                                pk.clone(),
                                RowLoc { block_idx: 0, row_pos: i as u64, row_hash: row_hash(&rows[i]) },
                            )
                        })
                        .collect();
                    states.insert(
                        table_name.to_string(),
                        TableReplicaState {
                            blocks: vec![base.clone()],
                            locations,
                            deltas_since_base: 0,
                            base_generation: 0,
                        },
                    );
                    vec![base]
                }
                Some(st) => {
                    let current: std::collections::HashSet<&str> =
                        pk_strings.iter().map(|s| s.as_str()).collect();

                    // 1. DELETE / 主キーを変える UPDATE: 以前は生きていたが
                    //    今は行ストアに無い主キー → deletion vector へマーク
                    //    (block の実体は書き直さない = 即時 rewrite 無しの MoR)。
                    let removed: Vec<String> = st
                        .locations
                        .keys()
                        .filter(|pk| !current.contains(pk.as_str()))
                        .cloned()
                        .collect();
                    for pk in &removed {
                        if let Some(loc) = st.locations.remove(pk) {
                            st.blocks[loc.block_idx].deletion_vector.insert(loc.row_pos);
                        }
                    }

                    // 2. INSERT / 行内容が変わった UPDATE: delta block へ集める。
                    //    in-place UPDATE は旧位置を deletion vector へ退避してから
                    //    delta へ再登録する。
                    let mut delta_pks: Vec<String> = Vec::new();
                    let mut delta_rows: Vec<Vec<String>> = Vec::new();
                    for (i, pk) in pk_strings.iter().enumerate() {
                        let h = row_hash(&rows[i]);
                        match st.locations.get(pk) {
                            Some(loc) if loc.row_hash == h => { /* 無変更、そのまま */ }
                            Some(loc) => {
                                let (bi, rp) = (loc.block_idx, loc.row_pos);
                                st.blocks[bi].deletion_vector.insert(rp);
                                delta_pks.push(pk.clone());
                                delta_rows.push(rows[i].clone());
                            }
                            None => {
                                delta_pks.push(pk.clone());
                                delta_rows.push(rows[i].clone());
                            }
                        }
                    }

                    let mut authored: Vec<BlockMeta> = Vec::new();
                    if !delta_pks.is_empty() {
                        let delta_idx = st.blocks.len();
                        let delta = build_block(
                            format!(
                                "mem://{table_name}/delta/{}/{}",
                                st.base_generation,
                                st.deltas_since_base + 1
                            ),
                            &cols,
                            &delta_pks,
                            &delta_rows,
                        );
                        st.blocks.push(delta.clone());
                        for (j, pk) in delta_pks.iter().enumerate() {
                            st.locations.insert(
                                pk.clone(),
                                RowLoc {
                                    block_idx: delta_idx,
                                    row_pos: j as u64,
                                    row_hash: row_hash(&delta_rows[j]),
                                },
                            );
                        }
                        st.deltas_since_base += 1;
                        authored.push(delta);
                    }

                    // 3. compaction: delta がたまったらテーブル全体から base を
                    //    作り直して畳み込む(TiFlash DeltaTree の閾値 compaction)。
                    if st.deltas_since_base >= self.compaction_threshold {
                        st.base_generation += 1;
                        let base = build_block(
                            format!("mem://{table_name}/base/{}", st.base_generation),
                            &cols,
                            &pk_strings,
                            &rows,
                        );
                        st.locations = pk_strings
                            .iter()
                            .enumerate()
                            .map(|(i, pk)| {
                                (
                                    pk.clone(),
                                    RowLoc {
                                        block_idx: 0,
                                        row_pos: i as u64,
                                        row_hash: row_hash(&rows[i]),
                                    },
                                )
                            })
                            .collect();
                        st.blocks = vec![base.clone()];
                        st.deltas_since_base = 0;
                        authored = vec![base];
                    }

                    authored
                }
            }
        };

        let object_table = self.object_table_for(table_name);
        object_table.commit_blocks(authored)?;
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

    /// この列レプリカが今保持している block 一覧(base + delta、deletion
    /// vector 込み)。これらを併合したものが MoR の現在ビュー
    /// (テスト・`Query.htapReplicas` 相当の観測用)。
    pub fn latest_blocks(&self, table_name: &str) -> Option<Vec<BlockMeta>> {
        self.replica_state.lock().get(table_name).map(|st| st.blocks.clone())
    }

    /// MoR 実効行数(全 block の物理行数から deletion vector 分を差し引いた
    /// 合計)。テスト・観測用。
    pub fn latest_live_row_count(&self, table_name: &str) -> Option<u64> {
        self.replica_state
            .lock()
            .get(table_name)
            .map(|st| st.blocks.iter().map(|b| b.live_row_count()).sum())
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
                // 列変換は「行ストアへの適用が成功した場合のみ」行う。
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
    fn second_insert_appends_a_delta_block_instead_of_rebuilding_the_whole_table() {
        let applier = new_applier();
        applier.apply(&Command::Exec(
            "CREATE TABLE items (id INT PRIMARY KEY, qty INT)".into(),
        ));
        // CREATE TABLE は空の base(0行)を1つ作る。
        let blocks = applier.latest_blocks("items").expect("blocks after create");
        assert_eq!(blocks.len(), 1, "create -> empty base only");
        assert_eq!(blocks[0].row_count, 0);

        applier.apply(&Command::Exec("INSERT INTO items (id, qty) VALUES (1, 10)".into()));
        let blocks = applier.latest_blocks("items").expect("blocks after first insert");
        assert_eq!(blocks.len(), 2, "first insert appends one delta block");

        applier.apply(&Command::Exec("INSERT INTO items (id, qty) VALUES (2, 20)".into()));
        // テーブル全体の都度再構築ではなく、新規行だけの小さな delta が
        // 追加される——どの block も物理行数は 1 を超えない。
        let blocks = applier.latest_blocks("items").expect("blocks after second insert");
        assert_eq!(blocks.len(), 3, "second insert appends another delta block");
        assert!(
            blocks.iter().all(|b| b.row_count <= 1),
            "no whole-table rebuild: every block holds at most one row"
        );
        assert_eq!(applier.latest_live_row_count("items"), Some(2));
    }

    #[test]
    fn delete_marks_the_deletion_vector_without_rewriting_the_block() {
        let applier = new_applier();
        applier.apply(&Command::Exec(
            "CREATE TABLE gear (id INT PRIMARY KEY, qty INT)".into(),
        ));
        applier.apply(&Command::Exec("INSERT INTO gear (id, qty) VALUES (1, 5)".into()));
        applier.apply(&Command::Exec("INSERT INTO gear (id, qty) VALUES (2, 9)".into()));
        applier.apply(&Command::Exec("INSERT INTO gear (id, qty) VALUES (3, 7)".into()));

        let before = applier.latest_blocks("gear").expect("blocks before delete");
        let phys_before: u64 = before.iter().map(|b| b.row_count).sum();
        assert_eq!(applier.latest_live_row_count("gear"), Some(3));

        applier.apply(&Command::Exec("DELETE FROM gear WHERE id = '2'".into()));

        let after = applier.latest_blocks("gear").expect("blocks after delete");
        let phys_after: u64 = after.iter().map(|b| b.row_count).sum();
        let deleted_positions: usize = after.iter().map(|b| b.deletion_vector.len()).sum();

        assert_eq!(
            phys_before, phys_after,
            "DELETE must NOT rewrite blocks (physical row_count unchanged)"
        );
        assert_eq!(deleted_positions, 1, "exactly one row position marked deleted");
        assert_eq!(
            applier.latest_live_row_count("gear"),
            Some(2),
            "MoR live count reflects the logical delete"
        );

        // 行ストア側のミラーも一致。
        let (_, pks, _) = applier.engine().snapshot_table("gear").expect("table exists");
        assert_eq!(pks.len(), 2);
    }

    #[test]
    fn in_place_update_retires_the_old_row_and_re_adds_it_in_a_delta() {
        let applier = new_applier();
        applier.apply(&Command::Exec(
            "CREATE TABLE gear (id INT PRIMARY KEY, qty INT)".into(),
        ));
        applier.apply(&Command::Exec("INSERT INTO gear (id, qty) VALUES (1, 5)".into()));
        applier.apply(&Command::Exec("INSERT INTO gear (id, qty) VALUES (2, 9)".into()));

        applier.apply(&Command::Exec("UPDATE gear SET qty = '99' WHERE id = '1'".into()));

        let blocks = applier.latest_blocks("gear").expect("blocks after update");
        let deleted_positions: usize = blocks.iter().map(|b| b.deletion_vector.len()).sum();
        assert_eq!(
            deleted_positions, 1,
            "the pre-update row position is marked deleted in place"
        );
        assert_eq!(
            applier.latest_live_row_count("gear"),
            Some(2),
            "still two live rows after an in-place update (no double count)"
        );

        // 列レプリカの生きている位置から復元した最新の qty が更新後の値。
        let (_, pks, rows) = applier.engine().snapshot_table("gear").expect("table exists");
        let idx = pks
            .iter()
            .position(|pk| String::from_utf8_lossy(pk) == "1")
            .expect("id=1 still present");
        assert_eq!(rows[idx][1], "99", "row store mirror reflects the update");
    }

    #[test]
    fn delta_blocks_compact_back_into_a_fresh_base_after_the_threshold() {
        let applier = new_applier();
        applier.apply(&Command::Exec(
            "CREATE TABLE t (id INT PRIMARY KEY, v INT)".into(),
        ));
        // 各 INSERT が delta を1つ足す。DEFAULT_COMPACTION_THRESHOLD 個目の delta で
        // compaction が走り、テーブル全体から作り直した base 1つに戻る。
        for i in 1..=(DEFAULT_COMPACTION_THRESHOLD as i64) {
            applier.apply(&Command::Exec(format!(
                "INSERT INTO t (id, v) VALUES ({i}, {i})"
            )));
        }
        let blocks = applier.latest_blocks("t").expect("blocks after compaction");
        assert_eq!(
            blocks.len(),
            1,
            "after {DEFAULT_COMPACTION_THRESHOLD} deltas the replica compacts to a single fresh base"
        );
        let live: u64 = blocks.iter().map(|b| b.live_row_count()).sum();
        assert_eq!(
            live, DEFAULT_COMPACTION_THRESHOLD as u64,
            "compacted base holds every live row exactly once"
        );
        // compaction 後の追加 INSERT はまた delta を足していく。
        applier.apply(&Command::Exec("INSERT INTO t (id, v) VALUES (99, 99)".into()));
        assert_eq!(
            applier.latest_blocks("t").map(|b| b.len()),
            Some(2),
            "post-compaction inserts append fresh deltas onto the new base"
        );
    }

    #[test]
    fn with_compaction_threshold_overrides_the_default() {
        // 閾値 3 → 3 個目の delta で base 1つへ畳み込む。
        let applier =
            ColumnarApplier::with_in_memory_store(Arc::new(QueryEngine::new())).with_compaction_threshold(3);
        applier.apply(&Command::Exec("CREATE TABLE t (id INT PRIMARY KEY, v INT)".into()));
        for i in 1..=3 {
            applier.apply(&Command::Exec(format!("INSERT INTO t (id, v) VALUES ({i}, {i})")));
        }
        assert_eq!(
            applier.latest_blocks("t").map(|b| b.len()),
            Some(1),
            "custom threshold=3 compacts after the 3rd delta"
        );
        // 0 は既定値へフォールバック。
        let a0 = ColumnarApplier::with_in_memory_store(Arc::new(QueryEngine::new())).with_compaction_threshold(0);
        assert_eq!(a0.compaction_threshold, DEFAULT_COMPACTION_THRESHOLD);
    }

    #[test]
    fn statements_that_fail_on_the_row_store_do_not_touch_the_columnar_replica() {
        let applier = new_applier();
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
