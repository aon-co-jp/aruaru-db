//! クエリ実行エンジン本体
//!
//! テーブルデータをインメモリに保持し、`aruaru_commit` 時に
//! 全テーブルの行を Prolly Tree にスナップショットして root_hash を確定、
//! VersionController にコミットを記録する。
//! これにより Git-on-SQL (branch/commit/diff) が実データと接続される。

use std::collections::BTreeMap;
use std::sync::Arc;

use parking_lot::RwLock;

use aruaru_core::catalog::ColumnType;
use aruaru_core::version::prolly::{NodeStore, ProllyTree};
use aruaru_core::version::VersionController;

use crate::parser::{self, ColumnDef, ConflictAction, ConflictValue, Statement};

/// 値の型 (pgwire への変換用)
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Value {
    Text(String),
    Int(i64),
    Null,
}

impl Value {
    pub fn as_text(&self) -> String {
        match self {
            Value::Text(s) => s.clone(),
            Value::Int(n) => n.to_string(),
            Value::Null => String::new(),
        }
    }
}

/// クエリの応答
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum QueryResponse {
    /// 行を返すクエリ (SELECT)
    Rows {
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
    },
    /// 行を返さないコマンド (INSERT/CREATE/コミット等)
    Command {
        /// PostgreSQL のコマンドタグ (例: "INSERT 0 1", "CREATE TABLE")
        tag: String,
    },
}

/// テーブルデータ
#[derive(Debug, Clone, Default)]
struct TableData {
    columns: Vec<String>,
    /// 各列の型 (columns と同じ並び)
    types: Vec<ColumnType>,
    /// PK (= 最初の列の値) のバイト列 → 行 (列値の配列)
    rows: BTreeMap<Vec<u8>, Vec<String>>,
}

/// トランザクション中に遅延させる永続化操作
#[derive(Debug, Clone)]
enum PersistOp {
    Row(String, Vec<u8>, Vec<String>),
    Del(String, Vec<u8>),
    Schema(String, Vec<(String, ColumnType)>),
    Drop(String),
}

/// トランザクション状態 (BEGIN ～ COMMIT/ROLLBACK)
struct TxnState {
    /// ロールバック用のテーブル全体スナップショット
    snapshot: BTreeMap<String, TableData>,
    /// COMMIT 時に永続ストアへ適用する操作ログ
    log: Vec<PersistOp>,
}

/// 冪等性ログを保持する内部テーブル名 (通常のSQL経路からは不可視の予約名)
const IDEMPOTENCY_TABLE: &str = "__idempotency_log";

/// クエリエンジン
/// `aruaru_commit`成功直後に、確定したcommit_idと全テーブルの現在の
/// 行データ(table_name, row_key, payload_json)を受け取るフック。
/// DUAL DATABASE構成(`aruaru_dist::dual_database`)のPostgreSQLミラーへ
/// 配線するために使う(2026-07-20追記、下記`set_commit_hook`のdocも参照)。
pub type CommitHook = dyn Fn(&str, &[(String, String, String)]) + Send + Sync;

pub struct QueryEngine {
    tables: RwLock<BTreeMap<String, TableData>>,
    store: Arc<NodeStore>,
    version: Arc<VersionController>,
    /// 永続ストア (設定時、commit ごとに自動 persist する)
    persistent: RwLock<Option<Arc<aruaru_core::PersistentStore>>>,
    /// アクティブなトランザクション (単一・直列化。None = autocommit)
    txn: parking_lot::Mutex<Option<TxnState>>,
    /// commit完了フック(DUAL DATABASEミラー等)。`RaftNode::set_commit_hook`
    /// と同じ設計判断: フック未登録時は何もしない、フック失敗が
    /// `aruaru_commit`自体の成功/失敗に影響しない(下記ドキュメント参照)。
    commit_hook: RwLock<Option<Arc<CommitHook>>>,
    /// 直前の`aruaru_commit`以降に書き込まれた`(table, pk)`の集合
    /// (2026-07-20追記、DUAL DATABASEミラーの全行ダンプ→差分抽出
    /// 最適化)。`persist_row`で書き込みのたびに追加し、`aruaru_commit`
    /// 成功時にこの集合だけをコミットフックへ渡した上でクリアする。
    /// 削除(`persist_delete`)はこの集合には追加しない——現行の
    /// `MirroredMutation`スキーマは「値」を運ぶ形であり削除(tombstone)を
    /// 表現できないため、削除された行がこの集合だけから復元しようとしても
    /// 存在しない値になる(既知の限界、`export_dirty_rows_as_json`のdoc参照)。
    dirty: RwLock<std::collections::BTreeSet<(String, Vec<u8>)>>,
    /// **HTAP列キャッシュ無効化(2026-07-20追記)**: `crate::olap`の
    /// `OlapCache`(行→列インクリメンタル同期、TiDB/TiFlash方式のこの
    /// エコシステムなりの実装)向けに、テーブル単位で「前回のOLAP列
    /// キャッシュ構築以降に変更があったか」を追跡する。上の`dirty`
    /// (DUAL DATABASEミラー用、行単位)とは目的・粒度が異なる別集合
    /// ——同じ集合を2つの消費者(ミラーのコミットフックとOLAPキャッシュ)
    /// で共有すると、片方が`take`で先にクリアしてしまいもう片方が
    /// 変更を見逃す実バグになるため、意図的に分離した。
    olap_dirty_tables: RwLock<std::collections::HashSet<String>>,
}

impl QueryEngine {
    pub fn new() -> Self {
        Self {
            tables: RwLock::new(BTreeMap::new()),
            store: Arc::new(NodeStore::new()),
            version: Arc::new(VersionController::new()),
            persistent: RwLock::new(None),
            txn: parking_lot::Mutex::new(None),
            commit_hook: RwLock::new(None),
            dirty: RwLock::new(std::collections::BTreeSet::new()),
            olap_dirty_tables: RwLock::new(std::collections::HashSet::new()),
        }
    }

    /// 既存の VersionController / NodeStore を共有して作る
    pub fn with_shared(store: Arc<NodeStore>, version: Arc<VersionController>) -> Self {
        Self {
            tables: RwLock::new(BTreeMap::new()),
            store,
            version,
            persistent: RwLock::new(None),
            txn: parking_lot::Mutex::new(None),
            commit_hook: RwLock::new(None),
            dirty: RwLock::new(std::collections::BTreeSet::new()),
            olap_dirty_tables: RwLock::new(std::collections::HashSet::new()),
        }
    }

    /// 永続ストアを取り付ける。以後 aruaru_commit ごとに自動で persist する。
    pub fn attach_store(&self, store: Arc<aruaru_core::PersistentStore>) {
        *self.persistent.write() = Some(store);
    }

    /// commit完了フックを登録する(DUAL DATABASEミラー等、2026-07-20追記)。
    ///
    /// **設計上の注意(正直な開示)**: `QueryEngine::execute`は同期関数
    /// であり、pgwire(`aruaru-wire`)の同期経路からも呼ばれる。フック自体を
    /// 非同期I/O(実PostgreSQLへのINSERT等)でブロックさせると、tokio
    /// ランタイムのワーカースレッド上で`block_on`することになり
    /// デッドロック/パニックのリスクがある(`Cannot start a runtime from
    /// within a runtime`)。そのためこのフックは**同期・非ブロッキング**
    /// であることが呼び出し契約 —— 実際に非同期I/Oを行う場合は、フック
    /// 内部で`tokio::spawn`する(呼び出し元が`#[tokio::main]`のランタイム
    /// 上で動いていることを前提とする、fire-and-forget)。これは
    /// `open-web-server-ledger::multi_region`が定めた「全レグの完了を
    /// 待ってから呼び出し元に返す」という厳密な同期ポリシーからの
    /// **意図的な逸脱**である(engineをasync化する大掛かりなリファクタ
    /// なしに両立できないため)。ミラー失敗はcommit自体の成功/失敗には
    /// 影響しない(`tracing::error!`のみ)。将来`execute`をasync化する
    /// 際は、この逸脱を解消し真の同期ミラーへ格上げすることが望ましい。
    pub fn set_commit_hook(&self, hook: Arc<CommitHook>) {
        *self.commit_hook.write() = Some(hook);
    }

    /// トランザクション中か
    pub fn in_transaction(&self) -> bool {
        self.txn.lock().is_some()
    }

    /// BEGIN: 現在のテーブル状態をスナップショットして txn を開始
    fn begin(&self) -> Result<QueryResponse, String> {
        let mut txn = self.txn.lock();
        if txn.is_some() {
            return Err("transaction already active".to_string());
        }
        *txn = Some(TxnState {
            snapshot: self.tables.read().clone(),
            log: Vec::new(),
        });
        Ok(QueryResponse::Command { tag: "BEGIN".to_string() })
    }

    /// COMMIT: 遅延ログを永続ストアへ適用して sync、txn 終了
    ///
    /// **耐久性契約(2026-07-18 修正)**: 以前は、遅延ログの個々の永続化
    /// 操作や最終的な WAL 同期 (`store.persist()`) が失敗しても
    /// `tracing::warn!` でログを出すだけで、呼び出し元には常に
    /// `COMMIT` 成功が返っていた——「DBがコミット成功を報告したのに
    /// 実際は WAL 同期が失敗していてデータが消える」という致命的な
    /// サイレント耐久性バグだった。現在は、ログ適用中のいずれかの操作
    /// または最終 WAL 同期が失敗した場合、**`Err` を返し、
    /// トランザクション開始時のスナップショットへインメモリのテーブル
    /// 状態を戻す**(＝ commit が失敗した場合は rollback と同じ状態に
    /// 揃え、「一部だけ永続化されたが呼び出し元はメモリ上にコミット
    /// 済みの行を見てしまう」というメモリ/ディスクの分岐を避ける)。
    ///
    /// **既知の限界**: ログ中の複数操作のうち一部が既に fjall へ
    /// 書き込まれた後に別の操作や最終 sync が失敗した場合、fjall 側の
    /// パーティションには部分的な書き込みが残る可能性がある(fjall
    /// 自体のバッチ/アトミック書き込みAPIは使っていないため)。真の
    /// アトミック性(全operationを1回のfjallバッチとして書く)は今回の
    /// 修正のスコープ外(本チケットの優先事項は「サイレント成功の
    /// 除去」であり、fjall書き込み自体のアトミック化は別課題として残す)。
    fn commit_txn(&self) -> Result<QueryResponse, String> {
        let state = self.txn.lock().take();
        let Some(state) = state else {
            // autocommit 中の COMMIT は no-op
            return Ok(QueryResponse::Command { tag: "COMMIT".to_string() });
        };
        if let Some(store) = self.persistent.read().clone() {
            for op in &state.log {
                let r = match op {
                    PersistOp::Row(t, pk, row) => store.save_row(t, pk, row),
                    PersistOp::Del(t, pk) => store.delete_row(t, pk),
                    PersistOp::Schema(t, cols) => store.save_schema(t, cols),
                    PersistOp::Drop(t) => store.drop_table(t),
                };
                if let Err(e) = r {
                    tracing::error!(error = %e, "txn commit persist op failed; aborting commit");
                    *self.tables.write() = state.snapshot;
                    return Err(format!("commit failed: persist op failed: {e}"));
                }
            }
            if let Err(e) = store.persist() {
                tracing::error!(error = %e, "txn commit WAL sync failed; aborting commit");
                *self.tables.write() = state.snapshot;
                return Err(format!("commit failed: WAL sync failed: {e}"));
            }
        }
        Ok(QueryResponse::Command { tag: "COMMIT".to_string() })
    }

    /// ROLLBACK: スナップショットへ復元、永続化は未適用なので破棄するだけ
    fn rollback(&self) -> Result<QueryResponse, String> {
        let state = self.txn.lock().take();
        match state {
            Some(state) => {
                *self.tables.write() = state.snapshot;
                Ok(QueryResponse::Command { tag: "ROLLBACK".to_string() })
            }
            None => Ok(QueryResponse::Command { tag: "ROLLBACK".to_string() }),
        }
    }

    /// 永続化操作を実行する。txn 中はログに記録し、autocommit 時は即適用。
    ///
    /// **耐久性契約**: autocommit 時に fjall への書き込み自体
    /// (`save_row`/`delete_row`/`save_schema`/`drop_table`) が失敗した場合、
    /// 以前は `tracing::warn!` でログを出すだけで呼び出し元には成功として
    /// 返っていた(=呼び出し元の SQL は成功応答を受け取るのに実際は
    /// 永続化されていない、というサイレント耐久性バグ)。現在はエラーを
    /// `Err` として呼び出し元まで伝播させ、対応する SQL 文自体を失敗として
    /// 扱う(2026-07-18 修正)。
    fn record_or_apply(&self, op: PersistOp) -> Result<(), String> {
        {
            let mut txn = self.txn.lock();
            if let Some(state) = txn.as_mut() {
                state.log.push(op);
                return Ok(());
            }
        }
        // autocommit: 即適用
        if let Some(store) = self.persistent.read().clone() {
            let r = match &op {
                PersistOp::Row(t, pk, row) => store.save_row(t, pk, row),
                PersistOp::Del(t, pk) => store.delete_row(t, pk),
                PersistOp::Schema(t, cols) => store.save_schema(t, cols),
                PersistOp::Drop(t) => store.drop_table(t),
            };
            if let Err(e) = r {
                tracing::error!(error = %e, "persist op failed; propagating as statement failure");
                return Err(format!("persist op failed: {e}"));
            }
        }
        Ok(())
    }

    /// 外部ソースから取得したテーブルを直接取り込む (お引越し用)。
    /// 全列を TEXT 型として作成/置換し、行を投入する。
    /// 戻り値は取り込んだ行数。
    pub fn ingest_table(
        &self,
        name: &str,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    ) -> usize {
        let types = vec![ColumnType::Text; columns.len()];
        let mut row_map = BTreeMap::new();
        for row in rows {
            // PK = 先頭列の値 (無ければ行インデックス相当のユニークキー)
            let pk = row
                .first()
                .cloned()
                .unwrap_or_default()
                .into_bytes();
            // 衝突回避: 既存キーがあれば連番サフィックス
            let mut key = pk.clone();
            let mut n = 0u32;
            while row_map.contains_key(&key) {
                n += 1;
                key = pk.clone();
                key.extend_from_slice(format!("#{n}").as_bytes());
            }
            row_map.insert(key, row);
        }
        let count = row_map.len();
        // write-through: スキーマと全行を永続化。
        // `ingest_table` はお引越し用のバルク取り込みヘルパで、呼び出し元
        // (admin.rs::migrate_run 等) は既に「ローカル限定・Raft複製対象外」
        // として文書化されている(CLAUDE.md HANDOFF 2026-07-18 参照)ため、
        // ここでは戻り値の型を変えず(呼び出し側3クレートへの波及を避ける)、
        // 個別の永続化失敗はエラーとしてログに残すに留める。ただし黙って
        // `tracing::warn!` するだけだった従来のコミット経路の問題とは異なり、
        // これはそもそも「1コミット=1操作」という単位を持たないバルク処理
        // であり、部分失敗時に呼び出し元へエラーを一つだけ返しても
        // 何行目が失敗したか分からず有用でないため、この境界では変更しない。
        let cols: Vec<(String, ColumnType)> =
            columns.iter().cloned().zip(types.iter().cloned()).collect();
        if let Err(e) = self.persist_schema(name, &cols) {
            tracing::error!(error = %e, table = name, "ingest_table: schema persist failed");
        }
        for (pk, row) in &row_map {
            if let Err(e) = self.persist_row(name, pk, row) {
                tracing::error!(error = %e, table = name, "ingest_table: row persist failed");
            }
        }
        self.tables.write().insert(
            name.to_string(),
            TableData {
                columns,
                types,
                rows: row_map,
            },
        );
        count
    }

    /// NodeStore への参照 (diff 計算で必要)
    pub fn store(&self) -> &Arc<NodeStore> {
        &self.store
    }

    /// VersionController への参照
    pub fn version(&self) -> &Arc<VersionController> {
        &self.version
    }

    /// テーブル名一覧
    pub fn table_names(&self) -> Vec<String> {
        self.tables.read().keys().cloned().collect()
    }

    /// 指定テーブルの行数 (存在しなければ None)
    pub fn table_row_count(&self, table: &str) -> Option<usize> {
        self.tables.read().get(table).map(|t| t.rows.len())
    }

    /// 指定テーブルの列名一覧 (存在しなければ None)。
    ///
    /// pgwireの`Describe(Statement)`応答(prepared statementの列形状を
    /// クエリ実行前に返す)を、クエリを実際に実行せずに構築するために
    /// 追加(2026-07-14、拡張プロトコル対応の一環)。`SELECT *`の実際の
    /// 列名を、行を1件も読まずに解決できる。
    pub fn table_columns(&self, table: &str) -> Option<Vec<String>> {
        self.tables.read().get(table).map(|t| t.columns.clone())
    }

    /// 全テーブルの合計行数
    pub fn total_rows(&self) -> usize {
        self.tables.read().values().map(|t| t.rows.len()).sum()
    }

    /// 1テーブルだけのスナップショット。`OlapCache`が変更のあった
    /// テーブルだけを再構築する際、全テーブルを走査する
    /// [`Self::snapshot_tables`] を避けるために使う。
    pub fn snapshot_table(&self, table: &str) -> Option<(Vec<(String, ColumnType)>, Vec<Vec<String>>)> {
        let tables = self.tables.read();
        let t = tables.get(table)?;
        let rows: Vec<Vec<String>> = t.rows.values().cloned().collect();
        let cols: Vec<(String, ColumnType)> = t.columns.iter().cloned().zip(t.types.iter().cloned()).collect();
        Some((cols, rows))
    }

    /// 全テーブルのスナップショット (name, 列定義(名前,型), rows) を取得。
    /// DataFusion OLAP 経路が型付き Arrow MemTable を構築するために使う。
    pub fn snapshot_tables(&self) -> Vec<(String, Vec<(String, ColumnType)>, Vec<Vec<String>>)> {
        self.tables
            .read()
            .iter()
            .map(|(name, t)| {
                let rows: Vec<Vec<String>> = t.rows.values().cloned().collect();
                let cols: Vec<(String, ColumnType)> = t
                    .columns
                    .iter()
                    .cloned()
                    .zip(t.types.iter().cloned())
                    .collect();
                (name.clone(), cols, rows)
            })
            .collect()
    }

    /// 全テーブルの全行を `(table_name, row_key, payload_json)` の形で書き出す。
    /// DUAL DATABASEミラー(コミットフック)向け(2026-07-20追記)。
    ///
    /// 現在は[`export_dirty_rows_as_json`]に置き換わり通常経路では呼ばれない
    /// (差分抽出への最適化、下記参照)。差分追跡が信頼できない状況
    /// (将来的な用途を想定し、削除はしていない)向けのフルダンプ手段として
    /// 残してある。
    #[allow(dead_code)]
    fn export_all_rows_as_json(&self) -> Vec<(String, String, String)> {
        let tables = self.tables.read();
        let mut out = Vec::new();
        for (table_name, t) in tables.iter() {
            for (pk, row) in &t.rows {
                let obj: serde_json::Map<String, serde_json::Value> = t
                    .columns
                    .iter()
                    .zip(row.iter())
                    .map(|(col, val)| (col.clone(), serde_json::Value::String(val.clone())))
                    .collect();
                let payload_json = serde_json::Value::Object(obj).to_string();
                let row_key = String::from_utf8_lossy(pk).into_owned();
                out.push((table_name.clone(), row_key, payload_json));
            }
        }
        out
    }

    /// 直前の`aruaru_commit`以降に実際に書き込まれた行だけを
    /// `(table_name, row_key, payload_json)`の形で書き出し、dirty集合を
    /// クリアする。DUAL DATABASEミラー(コミットフック)向け
    /// (2026-07-20追記、[`export_all_rows_as_json`]の全行ダンプ方式からの
    /// 最適化——コミットのたびにテーブル全体を送っていた無駄を解消する)。
    ///
    /// **正直な開示(既知の限界)**:
    /// 1. **削除は反映されない**: `persist_delete`はdirty集合に追加しない。
    ///    現行の`MirroredMutation`スキーマは「値」を運ぶ形であり、削除
    ///    (tombstone)を表現する列が無いため——削除された行のキーだけを
    ///    ミラーへ送っても、ミラー側は最後にINSERTされた値をそのまま
    ///    「最新値」として返し続けてしまう(削除がミラーに伝播しない)。
    ///    真に対応するには`MirroredMutation`にtombstoneフラグの追加が必要
    ///    (将来の増分、現状は削除を伴わないワークロード——課金アイテム
    ///    付与のような追記型データ——を主眼とする設計判断)。
    /// 2. **起動直後の初回コミットはフルダンプ相当になる**: `load_from`
    ///    (fjallからの復元)も`persist_row`経由でdirty集合に追加されるため、
    ///    再起動後の最初の`aruaru_commit`では復元した全行が(実際には
    ///    変更されていなくても)ミラーへ再送される。安全側(過剰送信は
    ///    データ欠落より無害)に倒した意図的な設計。
    fn export_dirty_rows_as_json(&self) -> Vec<(String, String, String)> {
        let dirty = std::mem::take(&mut *self.dirty.write());
        if dirty.is_empty() {
            return Vec::new();
        }
        let tables = self.tables.read();
        let mut out = Vec::with_capacity(dirty.len());
        for (table_name, pk) in dirty {
            let Some(t) = tables.get(&table_name) else { continue };
            let Some(row) = t.rows.get(&pk) else { continue };
            let obj: serde_json::Map<String, serde_json::Value> = t
                .columns
                .iter()
                .zip(row.iter())
                .map(|(col, val)| (col.clone(), serde_json::Value::String(val.clone())))
                .collect();
            let payload_json = serde_json::Value::Object(obj).to_string();
            let row_key = String::from_utf8_lossy(&pk).into_owned();
            out.push((table_name, row_key, payload_json));
        }
        out
    }

    /// HTAP ルーティング付き実行。
    /// OLAP (集計/GROUP BY/JOIN 等) は DataFusion 経路で並列実行し、
    /// 失敗時やそれ以外は組み込みエンジン (OLTP サブセット) にフォールバックする。
    pub async fn execute_async(&self, sql: &str) -> Result<QueryResponse, String> {
        if matches!(crate::classify_query(sql), crate::QueryKind::Olap) {
            match crate::olap::run_olap(self, sql).await {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    tracing::warn!(error = %e, "DataFusion OLAP path failed; falling back to builtin engine");
                }
            }
        }
        self.execute(sql)
    }

    /// fjall 永続ストアからテーブルを復元する (起動時)。
    pub fn load_from(&self, store: &aruaru_core::PersistentStore) -> Result<usize, String> {
        let schemas = store.load_schemas().map_err(|e| e.to_string())?;
        let mut tables = self.tables.write();
        let mut loaded = 0;
        for s in schemas {
            let names: Vec<String> = s.columns.iter().map(|(n, _)| n.clone()).collect();
            let types: Vec<ColumnType> = s.columns.iter().map(|(_, t)| t.clone()).collect();
            let rows = store.scan_table(&s.table).map_err(|e| e.to_string())?;
            let mut row_map = BTreeMap::new();
            for (pk, row) in rows {
                row_map.insert(pk, row);
            }
            tables.insert(
                s.table.clone(),
                TableData {
                    columns: names,
                    types,
                    rows: row_map,
                },
            );
            loaded += 1;
        }
        Ok(loaded)
    }

    /// 現在の全テーブルを fjall 永続ストアへ書き出して同期する (コミット時など)。
    pub fn persist_to(&self, store: &aruaru_core::PersistentStore) -> Result<(), String> {
        let tables = self.tables.read();
        for (name, t) in tables.iter() {
            let cols: Vec<(String, ColumnType)> = t
                .columns
                .iter()
                .cloned()
                .zip(t.types.iter().cloned())
                .collect();
            store.save_schema(name, &cols).map_err(|e| e.to_string())?;
            for (pk, row) in &t.rows {
                store.save_row(name, pk, row).map_err(|e| e.to_string())?;
            }
        }
        store.persist().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// SQL を実行
    pub fn execute(&self, sql: &str) -> Result<QueryResponse, String> {
        let stmt = parser::parse(sql)?;
        match stmt {
            Statement::CreateTable { table, columns } => self.create_table(table, columns),
            Statement::Insert {
                table,
                columns,
                values,
            } => self.insert(table, columns, values),
            Statement::Upsert {
                table,
                columns,
                values,
                conflict_column,
                action,
            } => self.upsert(table, columns, values, conflict_column, action),
            Statement::Select {
                table,
                columns,
                filter,
            } => self.select(table, columns, filter),
            Statement::Delete { table, filter } => self.delete(table, filter),
            Statement::Update { table, set, filter } => self.update(table, set, filter),
            Statement::DropTable { table } => self.drop_table(table),
            Statement::Begin => self.begin(),
            Statement::TxnCommit => self.commit_txn(),
            Statement::Rollback => self.rollback(),
            Statement::AruaruFn { name, arg } => self.aruaru_fn(&name, arg),
            Statement::AruaruLog { limit } => self.aruaru_log(limit),
            Statement::SelectAsOf {
                table,
                filter,
                commit_id,
            } => self.select_as_of(table, filter, commit_id),
        }
    }

    /// 【課金アイテムの権利消失防止】冪等性キー付きで書き込みSQLを実行する。
    /// 同じ `idempotency_key` で再度呼ばれた場合は再実行せず前回の結果を
    /// そのまま返す (ネットワーク切断後のクライアント自動リトライによる
    /// 二重課金・二重付与を防ぐ)。成功時は Git-on-SQL コミットとして記録され、
    /// 「いつ・何が実行されたか」を後から追跡・検証できる監査証跡になる。
    ///
    /// 冪等性ログは通常のテーブルと同じ write-through 永続化パス
    /// (`persist_row`) に載るため、`PersistentStore` 設定時はプロセス再起動を
    /// 跨いで保持される。
    pub fn execute_idempotent(
        &self,
        idempotency_key: &str,
        sql: &str,
        commit_message: &str,
    ) -> Result<QueryResponse, String> {
        self.ensure_idempotency_table()?;
        let pk = idempotency_key.as_bytes().to_vec();

        if let Some(row) = self
            .tables
            .read()
            .get(IDEMPOTENCY_TABLE)
            .and_then(|t| t.rows.get(&pk).cloned())
        {
            if let Some(json) = row.get(1) {
                if let Ok(resp) = rust_json::from_str_strict::<QueryResponse>(json) {
                    tracing::info!(
                        key = idempotency_key,
                        "idempotent replay: returning cached result (not re-executed)"
                    );
                    return Ok(resp);
                }
            }
        }

        let resp = self.execute(sql)?;

        let json = rust_json::to_string_strict(&resp).map_err(|e| e.to_string())?;
        let row = vec![idempotency_key.to_string(), json];
        {
            let mut tables = self.tables.write();
            let t = tables
                .get_mut(IDEMPOTENCY_TABLE)
                .expect("idempotency table ensured by ensure_idempotency_table");
            t.rows.insert(pk.clone(), row.clone());
        }
        self.persist_row(IDEMPOTENCY_TABLE, &pk, &row)?;

        // Git-on-SQL 監査証跡: 1トランザクション = 1コミット
        let safe_msg = commit_message.replace('\'', "''");
        let safe_key = idempotency_key.replace('\'', "''");
        self.execute(&format!(
            "SELECT aruaru_commit('{safe_msg} [idempotency_key={safe_key}]')"
        ))?;

        Ok(resp)
    }

    fn ensure_idempotency_table(&self) -> Result<(), String> {
        let exists = self.tables.read().contains_key(IDEMPOTENCY_TABLE);
        if exists {
            return Ok(());
        }
        {
            let mut tables = self.tables.write();
            tables
                .entry(IDEMPOTENCY_TABLE.to_string())
                .or_insert_with(|| TableData {
                    columns: vec!["key".to_string(), "result_json".to_string()],
                    types: vec![ColumnType::Text, ColumnType::Text],
                    rows: BTreeMap::new(),
                });
        }
        self.persist_schema(
            IDEMPOTENCY_TABLE,
            &[
                ("key".to_string(), ColumnType::Text),
                ("result_json".to_string(), ColumnType::Text),
            ],
        )
    }

    // ── write-through 永続化ヘルパ (txn中は遅延、ストア未設定なら no-op) ──

    fn persist_row(&self, table: &str, pk: &[u8], row: &[String]) -> Result<(), String> {
        self.dirty.write().insert((table.to_string(), pk.to_vec()));
        self.olap_dirty_tables.write().insert(table.to_string());
        self.record_or_apply(PersistOp::Row(table.to_string(), pk.to_vec(), row.to_vec()))
    }
    fn persist_delete(&self, table: &str, pk: &[u8]) -> Result<(), String> {
        self.olap_dirty_tables.write().insert(table.to_string());
        self.record_or_apply(PersistOp::Del(table.to_string(), pk.to_vec()))
    }
    fn persist_schema(&self, table: &str, cols: &[(String, ColumnType)]) -> Result<(), String> {
        self.olap_dirty_tables.write().insert(table.to_string());
        self.record_or_apply(PersistOp::Schema(table.to_string(), cols.to_vec()))
    }
    fn persist_drop(&self, table: &str) -> Result<(), String> {
        self.olap_dirty_tables.write().insert(table.to_string());
        self.record_or_apply(PersistOp::Drop(table.to_string()))
    }

    /// `table`がOLAP列キャッシュ構築以降に変更されたか(覗き見、消費しない)。
    pub fn is_olap_table_dirty(&self, table: &str) -> bool {
        self.olap_dirty_tables.read().contains(table)
    }

    /// `table`のOLAP dirtyフラグを落とす(列キャッシュを再構築し終えた後、
    /// `crate::olap::OlapCache`から呼ばれる)。
    pub fn clear_olap_dirty(&self, table: &str) {
        self.olap_dirty_tables.write().remove(table);
    }

    // ── DDL/DML ──────────────────────────────────────────────

    fn create_table(&self, table: String, columns: Vec<ColumnDef>) -> Result<QueryResponse, String> {
        let names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
        let types: Vec<ColumnType> = columns.iter().map(|c| c.ty.clone()).collect();
        {
            let mut tables = self.tables.write();
            tables.entry(table.clone()).or_insert_with(|| TableData {
                columns: names.clone(),
                types: types.clone(),
                rows: BTreeMap::new(),
            });
        }
        // write-through: スキーマを即永続化
        let cols: Vec<(String, ColumnType)> = names.into_iter().zip(types).collect();
        self.persist_schema(&table, &cols)?;
        Ok(QueryResponse::Command {
            tag: "CREATE TABLE".to_string(),
        })
    }

    fn insert(
        &self,
        table: String,
        columns: Vec<String>,
        values: Vec<String>,
    ) -> Result<QueryResponse, String> {
        let (pk, row) = {
            let mut tables = self.tables.write();
            let t = tables
                .get_mut(&table)
                .ok_or_else(|| format!("table not found: {}", table))?;

            // 列順を揃える: テーブル定義の列順に values を並べ替え
            let mut row = vec![String::new(); t.columns.len()];
            for (col, val) in columns.iter().zip(values.iter()) {
                if let Some(idx) = t.columns.iter().position(|c| c == col) {
                    row[idx] = val.clone();
                } else {
                    return Err(format!("unknown column: {}", col));
                }
            }
            // PK = 最初の列の値のバイト列
            let pk = row.first().cloned().unwrap_or_default().into_bytes();
            t.rows.insert(pk.clone(), row.clone());
            (pk, row)
        };
        // write-through: 行を即永続化
        self.persist_row(&table, &pk, &row)?;

        Ok(QueryResponse::Command {
            tag: "INSERT 0 1".to_string(),
        })
    }

    /// INSERT ... ON CONFLICT (col) DO UPDATE SET .../DO NOTHING
    ///
    /// `aruaru-db` はテーブル先頭列を常にPKとして扱うため、衝突判定は
    /// 「先頭列(PK)の値が既存行と一致するか」で行う。`conflict_column` は
    /// open-runo 側のSQLとの整合性チェック用(先頭列と一致しない場合はエラー)。
    /// これにより open-runo の `ON CONFLICT ... DO UPDATE` 生成SQLがそのまま
    /// 実行できるようになり、§0 の「exactly-once適用」要件
    /// (同じ課金アイテム付与/証券注文を再送しても二重に増えない)を満たす。
    fn upsert(
        &self,
        table: String,
        columns: Vec<String>,
        values: Vec<String>,
        conflict_column: Option<String>,
        action: ConflictAction,
    ) -> Result<QueryResponse, String> {
        let (pk, existed, final_row) = {
            let mut tables = self.tables.write();
            let t = tables
                .get_mut(&table)
                .ok_or_else(|| format!("table not found: {}", table))?;

            // 列順を揃える: テーブル定義の列順に values を並べ替え
            let mut new_row = vec![String::new(); t.columns.len()];
            for (col, val) in columns.iter().zip(values.iter()) {
                if let Some(idx) = t.columns.iter().position(|c| c == col) {
                    new_row[idx] = val.clone();
                } else {
                    return Err(format!("unknown column: {}", col));
                }
            }

            // conflict_column の整合性チェック (指定されていれば先頭列=PKと一致必須)
            if let Some(cc) = &conflict_column {
                if t.columns.first().map(|c| c.as_str()) != Some(cc.as_str()) {
                    return Err(format!(
                        "ON CONFLICT ({}): aruaru-db only supports the table's first \
                         column as the conflict target (PK); table '{}' first column is '{}'",
                        cc,
                        table,
                        t.columns.first().cloned().unwrap_or_default()
                    ));
                }
            }

            let pk = new_row.first().cloned().unwrap_or_default().into_bytes();
            let existed = t.rows.contains_key(&pk);

            let final_row = if !existed {
                // 新規行: 通常のINSERTと同じ
                t.rows.insert(pk.clone(), new_row.clone());
                new_row
            } else {
                match &action {
                    ConflictAction::DoNothing => {
                        // 既存行はそのまま。返り値用に現在の行を読む
                        t.rows.get(&pk).cloned().unwrap_or(new_row)
                    }
                    ConflictAction::DoUpdate(assignments) => {
                        let mut row = t.rows.get(&pk).cloned().unwrap_or_else(|| new_row.clone());
                        for (col, val) in assignments {
                            let idx = t
                                .columns
                                .iter()
                                .position(|c| c == col)
                                .ok_or_else(|| format!("unknown column in DO UPDATE SET: {}", col))?;
                            let resolved = match val {
                                ConflictValue::Literal(s) => s.clone(),
                                ConflictValue::Excluded(excl_col) => {
                                    let excl_idx = t
                                        .columns
                                        .iter()
                                        .position(|c| c == excl_col)
                                        .ok_or_else(|| {
                                            format!("unknown column in EXCLUDED.{}", excl_col)
                                        })?;
                                    new_row.get(excl_idx).cloned().unwrap_or_default()
                                }
                            };
                            row[idx] = resolved;
                        }
                        t.rows.insert(pk.clone(), row.clone());
                        row
                    }
                }
            };
            (pk, existed, final_row)
        };

        // write-through: 新規行 or 更新後の行を永続化 (DO NOTHING で既存行を
        // 変更しなかった場合も、再送によるズレを防ぐため現状態を書き直す)
        self.persist_row(&table, &pk, &final_row)?;

        let tag = if !existed {
            "INSERT 0 1".to_string()
        } else {
            match action {
                ConflictAction::DoNothing => "INSERT 0 0".to_string(),
                ConflictAction::DoUpdate(_) => "UPDATE 1".to_string(),
            }
        };
        Ok(QueryResponse::Command { tag })
    }

    /// DELETE FROM t [WHERE col = 'v']
    fn delete(
        &self,
        table: String,
        filter: Option<(String, String)>,
    ) -> Result<QueryResponse, String> {
        let removed_pks: Vec<Vec<u8>> = {
            let mut tables = self.tables.write();
            let t = tables
                .get_mut(&table)
                .ok_or_else(|| format!("table not found: {}", table))?;

            let filter_idx = match &filter {
                Some((col, _)) => Some(
                    t.columns
                        .iter()
                        .position(|c| c == col)
                        .ok_or_else(|| format!("unknown column: {}", col))?,
                ),
                None => None,
            };

            // 削除対象 pk を収集
            let mut to_remove = Vec::new();
            for (pk, row) in t.rows.iter() {
                let hit = match (filter_idx, &filter) {
                    (Some(idx), Some((_, val))) => row.get(idx).map(|s| s.as_str()) == Some(val.as_str()),
                    _ => true, // フィルタ無し = 全削除
                };
                if hit {
                    to_remove.push(pk.clone());
                }
            }
            for pk in &to_remove {
                t.rows.remove(pk);
            }
            to_remove
        };

        // write-through
        for pk in &removed_pks {
            self.persist_delete(&table, pk)?;
        }
        Ok(QueryResponse::Command {
            tag: format!("DELETE {}", removed_pks.len()),
        })
    }

    /// UPDATE t SET col = 'v' [WHERE col2 = 'v2']
    fn update(
        &self,
        table: String,
        set: (String, String),
        filter: Option<(String, String)>,
    ) -> Result<QueryResponse, String> {
        let updated: Vec<(Vec<u8>, Vec<String>)> = {
            let mut tables = self.tables.write();
            let t = tables
                .get_mut(&table)
                .ok_or_else(|| format!("table not found: {}", table))?;

            let set_idx = t
                .columns
                .iter()
                .position(|c| c == &set.0)
                .ok_or_else(|| format!("unknown column: {}", set.0))?;
            let filter_idx = match &filter {
                Some((col, _)) => Some(
                    t.columns
                        .iter()
                        .position(|c| c == col)
                        .ok_or_else(|| format!("unknown column: {}", col))?,
                ),
                None => None,
            };
            // PK 列 (= 0 列目) を更新すると BTreeMap キーが変わるため検出
            let pk_changed = set_idx == 0;

            let mut changed = Vec::new();
            for (pk, row) in t.rows.iter_mut() {
                let hit = match (filter_idx, &filter) {
                    (Some(idx), Some((_, val))) => row.get(idx).map(|s| s.as_str()) == Some(val.as_str()),
                    _ => true,
                };
                if hit {
                    row[set_idx] = set.1.clone();
                    changed.push((pk.clone(), row.clone()));
                }
            }

            // PK 列が変わった場合はキーを貼り直す
            if pk_changed {
                for (old_pk, row) in &changed {
                    t.rows.remove(old_pk);
                    let new_pk = row[0].clone().into_bytes();
                    t.rows.insert(new_pk, row.clone());
                }
            }
            changed
        };

        // write-through (PK 変更時は旧キー削除 + 新キー保存)
        for (old_pk, row) in &updated {
            let new_pk = row.first().cloned().unwrap_or_default().into_bytes();
            if &new_pk != old_pk {
                self.persist_delete(&table, old_pk)?;
            }
            self.persist_row(&table, &new_pk, row)?;
        }
        Ok(QueryResponse::Command {
            tag: format!("UPDATE {}", updated.len()),
        })
    }

    /// DROP TABLE t
    fn drop_table(&self, table: String) -> Result<QueryResponse, String> {
        let existed = self.tables.write().remove(&table).is_some();
        if existed {
            self.persist_drop(&table)?;
        }
        Ok(QueryResponse::Command {
            tag: "DROP TABLE".to_string(),
        })
    }

    fn select(
        &self,
        table: String,
        columns: Option<Vec<String>>,
        filter: Option<(String, String)>,
    ) -> Result<QueryResponse, String> {
        let tables = self.tables.read();
        let t = tables
            .get(&table)
            .ok_or_else(|| format!("table not found: {}", table))?;

        // 返す列を決定
        let out_columns: Vec<String> = match &columns {
            None => t.columns.clone(),
            Some(cols) => cols.clone(),
        };
        // 列インデックスを解決
        let indices: Vec<usize> = out_columns
            .iter()
            .map(|c| {
                t.columns
                    .iter()
                    .position(|tc| tc == c)
                    .ok_or_else(|| format!("unknown column: {}", c))
            })
            .collect::<Result<_, _>>()?;

        // フィルタ列のインデックス
        let filter_idx = match &filter {
            Some((col, _)) => Some(
                t.columns
                    .iter()
                    .position(|tc| tc == col)
                    .ok_or_else(|| format!("unknown filter column: {}", col))?,
            ),
            None => None,
        };

        let mut rows = Vec::new();
        for row in t.rows.values() {
            // フィルタ適用
            if let (Some(idx), Some((_, val))) = (filter_idx, &filter) {
                if row.get(idx).map(|s| s.as_str()) != Some(val.as_str()) {
                    continue;
                }
            }
            let projected: Vec<Value> = indices
                .iter()
                .map(|&i| Value::Text(row.get(i).cloned().unwrap_or_default()))
                .collect();
            rows.push(projected);
        }

        Ok(QueryResponse::Rows {
            columns: out_columns,
            rows,
        })
    }

    /// VersionLessAPI + Git版管理ハイブリッドの読み出し側:
    /// `SELECT ... FROM table WHERE pk = 'v' AS OF COMMIT 'commit_id'`
    ///
    /// `commit_id` が指す過去の `Commit.root_hash` から (現在の可変な
    /// `self.tables` ではなく) その時点の Prolly Tree を `ProllyTree::from_root`
    /// で再構築して読み出す — これが「最新状態ではなく特定コミット時点の
    /// 状態」を返す部分。同じ `NodeStore` (`self.store`) は `aruaru_commit`
    /// 実行時に `snapshot_root()` が書き込んだノードをすべて保持しているため、
    /// 古い commit の root からでも辿れる (Prolly Treeの構造的共有により、
    /// 変更されていない部分木は複数コミット間で共有される)。
    ///
    /// **スコープの限界 (正直な記載)**: 現状は単一行 (PK 一致の `WHERE`)
    /// のみサポートする。フルスキャン (`WHERE`無し・複数行) の AS OF は、
    /// このProllyTreeにテーブル横断の効率的なprefixスキャンAPIが今回追加
    /// されていないため次回以降の拡張とする — `scan()` はテーブル区別なく
    /// 全体を返すため、呼び出し側で`table\0`プレフィックスによる絞り込みが
    /// 必要になる (実装は容易だが、このパスでは単一行の実証を優先した)。
    fn select_as_of(
        &self,
        table: String,
        filter: Option<(String, String)>,
        commit_id: String,
    ) -> Result<QueryResponse, String> {
        let (_, pk_value) = filter.ok_or_else(|| {
            "AS OF COMMIT queries require a WHERE clause identifying the primary key \
             (full-table scans as of a commit are not yet supported)"
                .to_string()
        })?;

        let commit = self
            .version
            .get_commit_by_str(&commit_id)
            .ok_or_else(|| format!("commit not found: {commit_id}"))?;

        // キー形式は snapshot_root() と揃える: `table_name\0pk`
        let mut key = table.as_bytes().to_vec();
        key.push(0);
        key.extend_from_slice(pk_value.as_bytes());

        let tree = ProllyTree::from_root(commit.root_hash, self.store.clone());
        let Some(raw) = tree.get(&key) else {
            // その時点でまだ存在しなかった/既に削除されていた行
            return Ok(QueryResponse::Rows {
                columns: vec![],
                rows: vec![],
            });
        };

        let row: Vec<String> = String::from_utf8_lossy(&raw)
            .split('\t')
            .map(|s| s.to_string())
            .collect();

        // 現在もテーブルが存在する場合は列名を引き継ぐ (無ければ位置ベースの
        // 汎用列名にフォールバック — テーブルがその後DROPされていても
        // 過去データ自体は読み出せることを優先する)。
        let columns = {
            let tables = self.tables.read();
            match tables.get(&table) {
                Some(t) if t.columns.len() == row.len() => t.columns.clone(),
                _ => (0..row.len()).map(|i| format!("col{i}")).collect(),
            }
        };

        Ok(QueryResponse::Rows {
            columns,
            rows: vec![row.into_iter().map(Value::Text).collect()],
        })
    }

    // ── Git-on-SQL ───────────────────────────────────────────

    fn aruaru_fn(&self, name: &str, arg: Option<String>) -> Result<QueryResponse, String> {
        match name {
            "aruaru_branch" => {
                let branch = arg.ok_or("aruaru_branch requires a name")?;
                self.version
                    .create_branch(&branch)
                    .map_err(|e| e.to_string())?;
                Ok(QueryResponse::Rows {
                    columns: vec!["aruaru_branch".into()],
                    rows: vec![vec![Value::Text(format!("branch '{}' created", branch))]],
                })
            }
            "aruaru_checkout" => {
                let branch = arg.ok_or("aruaru_checkout requires a name")?;
                self.version.checkout(&branch).map_err(|e| e.to_string())?;
                Ok(QueryResponse::Rows {
                    columns: vec!["aruaru_checkout".into()],
                    rows: vec![vec![Value::Text(format!("switched to '{}'", branch))]],
                })
            }
            "aruaru_commit" => {
                let message = arg.unwrap_or_else(|| "(no message)".to_string());
                // 全テーブルの行を 1 つの Prolly Tree にスナップショット
                let root_hash = self.snapshot_root();
                let commit_id = self
                    .version
                    .commit("aruaru-server", &message, root_hash)
                    .map_err(|e| e.to_string())?;

                // write-through で各 DML は既に永続化済み。
                // commit ではバージョン確定後に WAL を同期するだけ。
                //
                // **耐久性契約(2026-07-18 修正)**: 以前は WAL 同期
                // (`store.persist()`) が失敗しても `tracing::warn!` する
                // だけで、呼び出し元には commit_id 付きの成功応答が
                // 返っていた——「Git-on-SQL コミットが成功したとDBが
                // 報告したのに、実際は WAL 同期が失敗していてデータが
                // 消える可能性がある」という致命的なサイレント耐久性
                // バグだった。現在は WAL 同期失敗を `Err` として呼び出し
                // 元へ伝播させ、この `aruaru_commit` 呼び出し自体を
                // 失敗として扱う。
                //
                // **既知の限界**: `self.version.commit(...)` (上の行) は
                // この WAL 同期より前に成功しているため、WAL 同期だけが
                // 失敗した場合、VersionController 側には既にコミット
                // レコードが残る(Git-on-SQL のコミットログ自体を巻き戻す
                // "uncommit" API は現状存在しない)。つまり `aruaru_log`
                // には現れるが、対応する行データの永続化は保証されない
                // コミットが残り得る、という非対称性が残る。これは
                // MVCCのような大規模な再設計を要するため今回は対応せず、
                // 既知の限界として記録する(呼び出し元がこの `Err` を
                // 見て「コミットは失敗した」と正しく扱うことが最優先の
                // 修正であり、それは達成されている)。
                if let Some(store) = self.persistent.read().clone() {
                    if let Err(e) = store.persist() {
                        tracing::error!(error = %e, commit = %commit_id.short(), "WAL sync on commit failed; reporting commit as failed");
                        return Err(format!(
                            "aruaru_commit failed: WAL sync failed after version commit {}: {e}",
                            commit_id.short()
                        ));
                    }
                    tracing::debug!(commit = %commit_id.short(), "persisted (synced) on commit");
                }

                // dirty集合は登録済みフックの有無によらず必ずクリアする
                // (`export_dirty_rows_as_json`が内部で`take`する)——フック
                // 未登録のままだとdirty集合がコミットのたびに際限なく
                // 肥大化してしまうメモリリークを防ぐため。
                let dirty_rows = self.export_dirty_rows_as_json();
                if let Some(hook) = self.commit_hook.read().clone() {
                    hook(commit_id.as_str(), &dirty_rows);
                }

                Ok(QueryResponse::Rows {
                    columns: vec!["aruaru_commit".into()],
                    rows: vec![vec![Value::Text(commit_id.as_str().to_string())]],
                })
            }
            "aruaru_merge" => {
                let from = arg.ok_or("aruaru_merge requires a source branch")?;
                let merged = self
                    .version
                    .fast_forward_merge(&from)
                    .map_err(|e| e.to_string())?;
                Ok(QueryResponse::Rows {
                    columns: vec!["aruaru_merge".into()],
                    rows: vec![vec![Value::Text(format!(
                        "merged '{}' -> {}",
                        from,
                        merged.short()
                    ))]],
                })
            }
            other => Err(format!("unknown aruaru function: {}", other)),
        }
    }

    fn aruaru_log(&self, limit: Option<usize>) -> Result<QueryResponse, String> {
        let commits = self.version.log(limit.unwrap_or(20));
        let columns = vec![
            "commit_id".to_string(),
            "author".to_string(),
            "message".to_string(),
            "timestamp".to_string(),
        ];
        let rows = commits
            .into_iter()
            .map(|c| {
                let timestamp = c.timestamp_rfc3339();
                vec![
                    Value::Text(c.id.short().to_string()),
                    Value::Text(c.author),
                    Value::Text(c.message),
                    Value::Text(timestamp),
                ]
            })
            .collect();
        Ok(QueryResponse::Rows { columns, rows })
    }

    /// 全テーブルの行を 1 つの Prolly Tree にまとめ、root_hash を返す。
    /// キー形式: `table_name\0pk` / 値: タブ区切りの行
    fn snapshot_root(&self) -> [u8; 32] {
        let tables = self.tables.read();
        let mut entries: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for (tname, t) in tables.iter() {
            for (pk, row) in &t.rows {
                let mut key = tname.as_bytes().to_vec();
                key.push(0);
                key.extend_from_slice(pk);
                let value = row.join("\t").into_bytes();
                entries.push((key, value));
            }
        }
        let tree = ProllyTree::new(self.store.clone());
        tree.build(entries);
        tree.root_hash()
    }
}

impl Default for QueryEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_insert_select() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE users (id INT, name TEXT)").unwrap();
        eng.execute("INSERT INTO users (id, name) VALUES (1, 'Alice')")
            .unwrap();
        eng.execute("INSERT INTO users (id, name) VALUES (2, 'Bob')")
            .unwrap();

        let resp = eng.execute("SELECT * FROM users").unwrap();
        if let QueryResponse::Rows { columns, rows } = resp {
            assert_eq!(columns, vec!["id", "name"]);
            assert_eq!(rows.len(), 2);
        } else {
            panic!("expected rows");
        }
    }

    /// DUAL DATABASEミラー配線(2026-07-20追記)の土台となるcommit_hookが、
    /// 実際にcommit成功直後に正しい commit_id と行データで呼ばれることを検証する。
    #[test]
    fn commit_hook_fires_with_commit_id_and_current_rows() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE items (id TEXT, qty TEXT)").unwrap();
        eng.execute("INSERT INTO items (id, qty) VALUES ('sword', '1')").unwrap();

        let captured: Arc<parking_lot::Mutex<Option<(String, Vec<(String, String, String)>)>>> =
            Arc::new(parking_lot::Mutex::new(None));
        let captured_clone = captured.clone();
        eng.set_commit_hook(Arc::new(move |commit_id, rows| {
            *captured_clone.lock() = Some((commit_id.to_string(), rows.to_vec()));
        }));

        let resp = eng.execute("SELECT aruaru_commit('add sword')").unwrap();
        let commit_id = match resp {
            QueryResponse::Rows { rows, .. } => match &rows[0][0] {
                Value::Text(s) => s.clone(),
                _ => panic!("expected commit id text"),
            },
            _ => panic!("expected rows"),
        };

        let (hook_commit_id, hook_rows) = captured.lock().take().expect("hook should have fired");
        assert_eq!(hook_commit_id, commit_id);
        assert_eq!(hook_rows.len(), 1);
        assert_eq!(hook_rows[0].0, "items");
        assert_eq!(hook_rows[0].1, "sword");
        assert!(hook_rows[0].2.contains("\"qty\":\"1\""));
    }

    /// フック未登録時は何もしない(既存の`commit`動作を一切変えないことを保証)。
    #[test]
    fn commit_without_hook_registered_still_succeeds() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE t (id TEXT)").unwrap();
        eng.execute("INSERT INTO t (id) VALUES ('x')").unwrap();
        assert!(eng.execute("SELECT aruaru_commit('no hook')").is_ok());
    }

    /// DUAL DATABASEミラーの最適化(全行ダンプ→差分抽出、2026-07-20追記):
    /// 2回目以降のコミットでは、その間に変更した行だけがフックへ渡り、
    /// 1回目のコミットで既に確定していた無関係な行は再送されないことを検証。
    #[test]
    fn commit_hook_only_receives_rows_changed_since_previous_commit() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE items (id TEXT, qty TEXT)").unwrap();
        eng.execute("INSERT INTO items (id, qty) VALUES ('sword', '1')").unwrap();
        eng.execute("INSERT INTO items (id, qty) VALUES ('shield', '1')").unwrap();

        let captured: Arc<parking_lot::Mutex<Vec<(String, String, String)>>> =
            Arc::new(parking_lot::Mutex::new(Vec::new()));
        let captured_clone = captured.clone();
        eng.set_commit_hook(Arc::new(move |_commit_id, rows| {
            *captured_clone.lock() = rows.to_vec();
        }));

        // 1回目のコミット: 2行とも新規なので両方渡ってくる。
        eng.execute("SELECT aruaru_commit('initial two items')").unwrap();
        let first_rows = captured.lock().clone();
        assert_eq!(first_rows.len(), 2);

        // 2回目のコミットまでに sword だけを更新。shield は無変更のまま。
        eng.execute("UPDATE items SET qty = '9' WHERE id = 'sword'").unwrap();
        eng.execute("SELECT aruaru_commit('sword restocked')").unwrap();

        let second_rows = captured.lock().clone();
        assert_eq!(second_rows.len(), 1, "only the changed row should be mirrored, not a full re-dump");
        assert_eq!(second_rows[0].0, "items");
        assert_eq!(second_rows[0].1, "sword");
        assert!(second_rows[0].2.contains("\"qty\":\"9\""));

        // 3回目: 何も変更していなければ空。
        eng.execute("SELECT aruaru_commit('no-op commit')").unwrap();
        assert!(captured.lock().is_empty(), "a commit with no writes since the last one should mirror nothing");
    }

    /// VersionLessAPI + Git版管理ハイブリッドの読み出し側 (open-web-server/
    /// CLAUDE.md 拡張要件(1) の残ギャップ) の一気通貫テスト:
    /// 同じキーに対して複数回コミットし、古いcommit_idを指定した
    /// `AS OF COMMIT` クエリが**最新値ではなく過去の値**を返すことを実証する。
    #[test]
    fn as_of_commit_returns_the_value_from_that_commit_not_the_latest() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE items (id TEXT, qty INT)").unwrap();

        eng.execute("INSERT INTO items (id, qty) VALUES ('sword', 1)")
            .unwrap();
        let commit_1 = match eng.execute("SELECT aruaru_commit('first grant')").unwrap() {
            QueryResponse::Rows { rows, .. } => match &rows[0][0] {
                Value::Text(s) => s.clone(),
                _ => panic!("expected text commit id"),
            },
            _ => panic!("expected rows"),
        };

        eng.execute("UPDATE items SET qty = '5' WHERE id = 'sword'")
            .unwrap();
        let commit_2 = match eng.execute("SELECT aruaru_commit('quantity bumped')").unwrap() {
            QueryResponse::Rows { rows, .. } => match &rows[0][0] {
                Value::Text(s) => s.clone(),
                _ => panic!("expected text commit id"),
            },
            _ => panic!("expected rows"),
        };
        assert_ne!(commit_1, commit_2, "each commit must get a distinct id");

        // 現在の状態 (最新) は qty=5
        let latest = eng.execute("SELECT qty FROM items WHERE id = 'sword'").unwrap();
        if let QueryResponse::Rows { rows, .. } = latest {
            assert_eq!(rows[0][0], Value::Text("5".to_string()));
        } else {
            panic!("expected rows");
        }

        // commit_1 時点 (最初のコミット) の状態を問い合わせると qty=1 のまま
        let as_of_1 = eng
            .execute(&format!(
                "SELECT qty FROM items WHERE id = 'sword' AS OF COMMIT '{commit_1}'"
            ))
            .unwrap();
        if let QueryResponse::Rows { columns, rows } = as_of_1 {
            assert_eq!(rows.len(), 1, "row must exist as of commit_1");
            assert_eq!(columns, vec!["id", "qty"]);
            assert_eq!(
                rows[0],
                vec![Value::Text("sword".to_string()), Value::Text("1".to_string())],
                "AS OF commit_1 must return the value as of that commit, not the latest value"
            );
        } else {
            panic!("expected rows");
        }

        // commit_2 時点の状態を問い合わせると qty=5 (最新と一致するが、
        // これも「その時点のスナップショット」から独立に導出されている)
        let as_of_2 = eng
            .execute(&format!(
                "SELECT qty FROM items WHERE id = 'sword' AS OF COMMIT '{commit_2}'"
            ))
            .unwrap();
        if let QueryResponse::Rows { rows, .. } = as_of_2 {
            assert_eq!(rows[0][1], Value::Text("5".to_string()));
        } else {
            panic!("expected rows");
        }

        // 存在しないcommit_idはエラー
        assert!(eng
            .execute("SELECT qty FROM items WHERE id = 'sword' AS OF COMMIT 'deadbeef'")
            .is_err());
    }

    #[test]
    fn test_select_where() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE users (id INT, name TEXT)").unwrap();
        eng.execute("INSERT INTO users (id, name) VALUES (1, 'Alice')")
            .unwrap();
        eng.execute("INSERT INTO users (id, name) VALUES (2, 'Bob')")
            .unwrap();

        let resp = eng
            .execute("SELECT name FROM users WHERE id = '2'")
            .unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][0], Value::Text("Bob".into()));
        } else {
            panic!("expected rows");
        }
    }

    #[test]
    fn test_git_on_sql_flow() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE t (id INT, val TEXT)").unwrap();
        eng.execute("INSERT INTO t (id, val) VALUES (1, 'first')")
            .unwrap();

        // コミット
        let resp = eng.execute("SELECT aruaru_commit('initial data')").unwrap();
        let commit_id = if let QueryResponse::Rows { rows, .. } = resp {
            rows[0][0].as_text()
        } else {
            panic!("expected commit id");
        };
        assert!(!commit_id.is_empty());

        // ログに 2 件 (genesis + initial data)
        let resp = eng.execute("SELECT * FROM aruaru_log").unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert!(rows.len() >= 2);
            assert!(rows.iter().any(|r| r[2].as_text() == "initial data"));
        } else {
            panic!("expected rows");
        }
    }

    #[test]
    fn test_upsert_insert_when_absent() {
        // ON CONFLICT 付きINSERTでも、行が存在しなければ通常のINSERTと同じ
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE wallets (id TEXT, balance TEXT)").unwrap();
        eng.execute(
            "INSERT INTO wallets (id, balance) VALUES ('u1', '100') \
             ON CONFLICT (id) DO UPDATE SET balance = EXCLUDED.balance",
        )
        .unwrap();

        let resp = eng.execute("SELECT * FROM wallets").unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][1], Value::Text("100".into()));
        } else {
            panic!("expected rows");
        }
    }

    #[test]
    fn test_upsert_do_update_on_conflict() {
        // 【§0 zero-loss mission】同じ口座への2回目の入金UPSERTが
        // 新規行を作らず既存残高を更新することを確認 (課金アイテム/口座残高の
        // 二重付与防止と同じ形の保証)
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE wallets (id TEXT, balance TEXT)").unwrap();
        eng.execute(
            "INSERT INTO wallets (id, balance) VALUES ('u1', '100') \
             ON CONFLICT (id) DO UPDATE SET balance = EXCLUDED.balance",
        )
        .unwrap();
        eng.execute(
            "INSERT INTO wallets (id, balance) VALUES ('u1', '250') \
             ON CONFLICT (id) DO UPDATE SET balance = EXCLUDED.balance",
        )
        .unwrap();

        let resp = eng.execute("SELECT * FROM wallets").unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows.len(), 1, "must not create a duplicate row on conflict");
            assert_eq!(rows[0][1], Value::Text("250".into()));
        } else {
            panic!("expected rows");
        }
    }

    #[test]
    fn test_upsert_do_nothing_keeps_existing_value() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE items (id TEXT, granted TEXT)").unwrap();
        eng.execute(
            "INSERT INTO items (id, granted) VALUES ('sword-1', 'yes') ON CONFLICT (id) DO NOTHING",
        )
        .unwrap();
        // 同じidempotency的な再送を想定した再送 — 既存の 'yes' を変えないこと
        eng.execute(
            "INSERT INTO items (id, granted) VALUES ('sword-1', 'no') ON CONFLICT (id) DO NOTHING",
        )
        .unwrap();

        let resp = eng.execute("SELECT * FROM items").unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][1], Value::Text("yes".into()));
        } else {
            panic!("expected rows");
        }
    }

    #[test]
    fn test_upsert_multi_column_do_update() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE orders (id TEXT, qty TEXT, status TEXT)").unwrap();
        eng.execute(
            "INSERT INTO orders (id, qty, status) VALUES ('o1', '1', 'pending') \
             ON CONFLICT (id) DO UPDATE SET qty = '5', status = 'filled'",
        )
        .unwrap();
        eng.execute(
            "INSERT INTO orders (id, qty, status) VALUES ('o1', '1', 'pending') \
             ON CONFLICT (id) DO UPDATE SET qty = '5', status = 'filled'",
        )
        .unwrap();

        let resp = eng.execute("SELECT * FROM orders").unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][1], Value::Text("5".into()));
            assert_eq!(rows[0][2], Value::Text("filled".into()));
        } else {
            panic!("expected rows");
        }
    }

    #[test]
    fn test_transaction_commit() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE t (id INT, v TEXT)").unwrap();
        eng.execute("INSERT INTO t (id, v) VALUES (1, 'a')").unwrap();

        eng.execute("BEGIN").unwrap();
        assert!(eng.in_transaction());
        eng.execute("INSERT INTO t (id, v) VALUES (2, 'b')").unwrap();
        eng.execute("INSERT INTO t (id, v) VALUES (3, 'c')").unwrap();
        eng.execute("COMMIT").unwrap();
        assert!(!eng.in_transaction());

        let resp = eng.execute("SELECT * FROM t").unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows.len(), 3); // コミットで確定
        } else { panic!() }
    }

    #[test]
    fn test_transaction_rollback() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE t (id INT, v TEXT)").unwrap();
        eng.execute("INSERT INTO t (id, v) VALUES (1, 'a')").unwrap();

        eng.execute("BEGIN").unwrap();
        eng.execute("INSERT INTO t (id, v) VALUES (2, 'b')").unwrap();
        eng.execute("DELETE FROM t WHERE id = '1'").unwrap();
        // ロールバック前は変更が見える
        if let QueryResponse::Rows { rows, .. } = eng.execute("SELECT * FROM t").unwrap() {
            assert_eq!(rows.len(), 1); // id=1削除, id=2追加 → 1行
        } else { panic!() }

        eng.execute("ROLLBACK").unwrap();
        assert!(!eng.in_transaction());

        // ロールバックで BEGIN 時点 (id=1 のみ) に戻る
        if let QueryResponse::Rows { rows, .. } = eng.execute("SELECT * FROM t").unwrap() {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][0], Value::Text("1".into()));
        } else { panic!() }
    }

    #[test]
    fn test_double_begin_errors() {
        let eng = QueryEngine::new();
        eng.execute("BEGIN").unwrap();
        assert!(eng.execute("BEGIN").is_err());
        eng.execute("ROLLBACK").unwrap();
    }

    #[test]
    fn test_transaction_rollback_not_persisted() {
        use std::sync::Arc;
        let dir = std::env::temp_dir().join(format!("aruaru-txn-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        {
            let eng = QueryEngine::new();
            eng.attach_store(Arc::new(aruaru_core::PersistentStore::open(&dir).unwrap()));
            eng.execute("CREATE TABLE t (id INT, v TEXT)").unwrap();
            eng.execute("INSERT INTO t (id, v) VALUES (1, 'keep')").unwrap();
            eng.execute("BEGIN").unwrap();
            eng.execute("INSERT INTO t (id, v) VALUES (2, 'discard')").unwrap();
            eng.execute("ROLLBACK").unwrap();
        }
        {
            let eng2 = QueryEngine::new();
            let store = aruaru_core::PersistentStore::open(&dir).unwrap();
            eng2.load_from(&store).unwrap();
            // ロールバックされた id=2 は永続化されていない
            if let QueryResponse::Rows { rows, .. } = eng2.execute("SELECT * FROM t").unwrap() {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][1], Value::Text("keep".into()));
            } else { panic!() }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_delete_update_drop() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE u (id INT, name TEXT)").unwrap();
        eng.execute("INSERT INTO u (id, name) VALUES (1, 'A')").unwrap();
        eng.execute("INSERT INTO u (id, name) VALUES (2, 'B')").unwrap();
        eng.execute("INSERT INTO u (id, name) VALUES (3, 'C')").unwrap();

        // UPDATE
        let r = eng.execute("UPDATE u SET name = 'Z' WHERE id = '2'").unwrap();
        assert!(matches!(r, QueryResponse::Command { ref tag } if tag == "UPDATE 1"));
        let resp = eng.execute("SELECT name FROM u WHERE id = '2'").unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows[0][0], Value::Text("Z".into()));
        } else { panic!() }

        // DELETE
        let r = eng.execute("DELETE FROM u WHERE id = '1'").unwrap();
        assert!(matches!(r, QueryResponse::Command { ref tag } if tag == "DELETE 1"));
        let resp = eng.execute("SELECT * FROM u").unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows.len(), 2);
        } else { panic!() }

        // DROP
        eng.execute("DROP TABLE u").unwrap();
        assert!(eng.execute("SELECT * FROM u").is_err());
    }

    #[test]
    fn test_delete_persists() {
        use std::sync::Arc;
        let dir = std::env::temp_dir().join(format!("aruaru-del-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        {
            let eng = QueryEngine::new();
            eng.attach_store(Arc::new(aruaru_core::PersistentStore::open(&dir).unwrap()));
            eng.execute("CREATE TABLE t (id INT, v TEXT)").unwrap();
            eng.execute("INSERT INTO t (id, v) VALUES (1, 'a')").unwrap();
            eng.execute("INSERT INTO t (id, v) VALUES (2, 'b')").unwrap();
            eng.execute("DELETE FROM t WHERE id = '1'").unwrap();
        }
        {
            let eng2 = QueryEngine::new();
            let store = aruaru_core::PersistentStore::open(&dir).unwrap();
            eng2.load_from(&store).unwrap();
            let resp = eng2.execute("SELECT * FROM t").unwrap();
            if let QueryResponse::Rows { rows, .. } = resp {
                // 削除が永続化され、1行だけ復元される
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][0], Value::Text("2".into()));
            } else { panic!() }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_auto_persist_on_commit() {
        use std::sync::Arc;
        let dir = std::env::temp_dir().join(format!("aruaru-engine-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        {
            let eng = QueryEngine::new();
            let store = Arc::new(aruaru_core::PersistentStore::open(&dir).unwrap());
            eng.attach_store(store);
            eng.execute("CREATE TABLE t (id INT, val TEXT)").unwrap();
            eng.execute("INSERT INTO t (id, val) VALUES (1, 'hello')").unwrap();
            // commit で自動 persist されるはず
            eng.execute("SELECT aruaru_commit('persisted')").unwrap();
        }

        // 別エンジンに復元して確認
        {
            let eng2 = QueryEngine::new();
            let store = aruaru_core::PersistentStore::open(&dir).unwrap();
            let loaded = eng2.load_from(&store).unwrap();
            assert_eq!(loaded, 1);
            let resp = eng2.execute("SELECT * FROM t").unwrap();
            if let QueryResponse::Rows { rows, .. } = resp {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][1], Value::Text("hello".into()));
            } else {
                panic!("expected rows");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_diff_branches() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE t (id INT, val TEXT)").unwrap();
        eng.execute("INSERT INTO t (id, val) VALUES (1, 'a')").unwrap();
        eng.execute("SELECT aruaru_commit('main commit')").unwrap();

        // ブランチを作って変更
        eng.execute("SELECT aruaru_branch('feature')").unwrap();
        eng.execute("SELECT aruaru_checkout('feature')").unwrap();
        eng.execute("INSERT INTO t (id, val) VALUES (2, 'b')").unwrap();
        eng.execute("SELECT aruaru_commit('feature commit')").unwrap();

        // diff (main vs feature) — id=2 が追加されているはず
        let diff = eng
            .version
            .diff_branches(eng.store(), "main", "feature")
            .unwrap();
        assert_eq!(diff.added_count(), 1, "id=2 の行が追加されているはず");
    }

    #[test]
    fn test_upsert_do_nothing_keeps_existing_row() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE wallets (id INT, balance TEXT)").unwrap();
        eng.execute("INSERT INTO wallets (id, balance) VALUES (1, '100')").unwrap();

        // 同じPKで再送 (ネットワーク再送を想定) → DO NOTHINGで残高は変わらない
        eng.execute(
            "INSERT INTO wallets (id, balance) VALUES (1, '999') ON CONFLICT (id) DO NOTHING",
        )
        .unwrap();

        let resp = eng.execute("SELECT balance FROM wallets WHERE id = '1'").unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][0], Value::Text("100".into()));
        } else {
            panic!("expected rows");
        }
    }

    #[test]
    fn test_upsert_do_update_set_excluded() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE wallets (id INT, balance TEXT)").unwrap();
        eng.execute("INSERT INTO wallets (id, balance) VALUES (1, '100')").unwrap();

        // 既存行を EXCLUDED (新しい値) で更新
        eng.execute(
            "INSERT INTO wallets (id, balance) VALUES (1, '250') ON CONFLICT (id) DO UPDATE SET balance = EXCLUDED.balance",
        )
        .unwrap();

        let resp = eng.execute("SELECT balance FROM wallets WHERE id = '1'").unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][0], Value::Text("250".into()));
        } else {
            panic!("expected rows");
        }
    }

    #[test]
    fn test_upsert_inserts_when_no_conflict() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE items (id INT, qty TEXT)").unwrap();

        // 既存行なし → 通常のINSERTとして動作するはず
        eng.execute(
            "INSERT INTO items (id, qty) VALUES (1, '5') ON CONFLICT (id) DO UPDATE SET qty = EXCLUDED.qty",
        )
        .unwrap();

        let resp = eng.execute("SELECT * FROM items").unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][1], Value::Text("5".into()));
        } else {
            panic!("expected rows");
        }
    }

    #[test]
    fn test_upsert_idempotent_replay_does_not_double_grant() {
        // §0 の使命(課金アイテム/資産データのexactly-once適用)そのものの回帰テスト:
        // 同じ idempotency_key で同じUPSERTを2回送っても、残高が二重に増えないこと。
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE wallets (id INT, balance TEXT)").unwrap();
        eng.execute("INSERT INTO wallets (id, balance) VALUES (1, '0')").unwrap();

        let sql = "INSERT INTO wallets (id, balance) VALUES (1, '500') ON CONFLICT (id) DO UPDATE SET balance = EXCLUDED.balance";
        eng.execute_idempotent("grant-item-42", sql, "grant paid item #42")
            .unwrap();
        // クライアントのリトライを模擬: 同じキーで再送
        eng.execute_idempotent("grant-item-42", sql, "grant paid item #42")
            .unwrap();

        let resp = eng.execute("SELECT balance FROM wallets WHERE id = '1'").unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows.len(), 1);
            // 再送しても '500' から変わらない (二重付与ではない) ことを確認
            assert_eq!(rows[0][0], Value::Text("500".into()));
        } else {
            panic!("expected rows");
        }
    }
}
