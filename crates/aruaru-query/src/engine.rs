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
pub struct QueryEngine {
    tables: RwLock<BTreeMap<String, TableData>>,
    store: Arc<NodeStore>,
    version: Arc<VersionController>,
    /// 永続ストア (設定時、commit ごとに自動 persist する)
    persistent: RwLock<Option<Arc<aruaru_core::PersistentStore>>>,
    /// アクティブなトランザクション (単一・直列化。None = autocommit)
    txn: parking_lot::Mutex<Option<TxnState>>,
}

impl QueryEngine {
    pub fn new() -> Self {
        Self {
            tables: RwLock::new(BTreeMap::new()),
            store: Arc::new(NodeStore::new()),
            version: Arc::new(VersionController::new()),
            persistent: RwLock::new(None),
            txn: parking_lot::Mutex::new(None),
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
        }
    }

    /// 永続ストアを取り付ける。以後 aruaru_commit ごとに自動で persist する。
    pub fn attach_store(&self, store: Arc<aruaru_core::PersistentStore>) {
        *self.persistent.write() = Some(store);
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
                    tracing::warn!(error = %e, "txn commit persist op failed");
                }
            }
            if let Err(e) = store.persist() {
                tracing::warn!(error = %e, "txn commit WAL sync failed");
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
    fn record_or_apply(&self, op: PersistOp) {
        {
            let mut txn = self.txn.lock();
            if let Some(state) = txn.as_mut() {
                state.log.push(op);
                return;
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
                tracing::warn!(error = %e, "persist op failed");
            }
        }
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
        // write-through: スキーマと全行を永続化
        let cols: Vec<(String, ColumnType)> =
            columns.iter().cloned().zip(types.iter().cloned()).collect();
        self.persist_schema(name, &cols);
        for (pk, row) in &row_map {
            self.persist_row(name, pk, row);
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

    /// 全テーブルの合計行数
    pub fn total_rows(&self) -> usize {
        self.tables.read().values().map(|t| t.rows.len()).sum()
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
        self.ensure_idempotency_table();
        let pk = idempotency_key.as_bytes().to_vec();

        if let Some(row) = self
            .tables
            .read()
            .get(IDEMPOTENCY_TABLE)
            .and_then(|t| t.rows.get(&pk).cloned())
        {
            if let Some(json) = row.get(1) {
                if let Ok(resp) = serde_json::from_str::<QueryResponse>(json) {
                    tracing::info!(
                        key = idempotency_key,
                        "idempotent replay: returning cached result (not re-executed)"
                    );
                    return Ok(resp);
                }
            }
        }

        let resp = self.execute(sql)?;

        let json = serde_json::to_string(&resp).map_err(|e| e.to_string())?;
        let row = vec![idempotency_key.to_string(), json];
        {
            let mut tables = self.tables.write();
            let t = tables
                .get_mut(IDEMPOTENCY_TABLE)
                .expect("idempotency table ensured by ensure_idempotency_table");
            t.rows.insert(pk.clone(), row.clone());
        }
        self.persist_row(IDEMPOTENCY_TABLE, &pk, &row);

        // Git-on-SQL 監査証跡: 1トランザクション = 1コミット
        let safe_msg = commit_message.replace('\'', "''");
        let safe_key = idempotency_key.replace('\'', "''");
        self.execute(&format!(
            "SELECT aruaru_commit('{safe_msg} [idempotency_key={safe_key}]')"
        ))?;

        Ok(resp)
    }

    fn ensure_idempotency_table(&self) {
        let exists = self.tables.read().contains_key(IDEMPOTENCY_TABLE);
        if exists {
            return;
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
        );
    }

    // ── write-through 永続化ヘルパ (txn中は遅延、ストア未設定なら no-op) ──

    fn persist_row(&self, table: &str, pk: &[u8], row: &[String]) {
        self.record_or_apply(PersistOp::Row(table.to_string(), pk.to_vec(), row.to_vec()));
    }
    fn persist_delete(&self, table: &str, pk: &[u8]) {
        self.record_or_apply(PersistOp::Del(table.to_string(), pk.to_vec()));
    }
    fn persist_schema(&self, table: &str, cols: &[(String, ColumnType)]) {
        self.record_or_apply(PersistOp::Schema(table.to_string(), cols.to_vec()));
    }
    fn persist_drop(&self, table: &str) {
        self.record_or_apply(PersistOp::Drop(table.to_string()));
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
        self.persist_schema(&table, &cols);
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
        self.persist_row(&table, &pk, &row);

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
        self.persist_row(&table, &pk, &final_row);

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
            self.persist_delete(&table, pk);
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
                self.persist_delete(&table, old_pk);
            }
            self.persist_row(&table, &new_pk, row);
        }
        Ok(QueryResponse::Command {
            tag: format!("UPDATE {}", updated.len()),
        })
    }

    /// DROP TABLE t
    fn drop_table(&self, table: String) -> Result<QueryResponse, String> {
        let existed = self.tables.write().remove(&table).is_some();
        if existed {
            self.persist_drop(&table);
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
                if let Some(store) = self.persistent.read().clone() {
                    if let Err(e) = store.persist() {
                        tracing::warn!(error = %e, "WAL sync on commit failed");
                    } else {
                        tracing::debug!(commit = %commit_id.short(), "persisted (synced) on commit");
                    }
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
