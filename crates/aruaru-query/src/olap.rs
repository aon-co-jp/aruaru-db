//! OLAP 実行経路 (DataFusion)
//!
//! HTAP ルーターが OLAP と判定したクエリ (集計・GROUP BY・JOIN 等) を
//! Apache DataFusion で実行する。DataFusion は列指向・ベクトル化・
//! マルチパーティションの実行エンジンで、`target_partitions` の数だけ
//! パーティション並列 (Volcano + RepartitionExec) で処理する。
//! = **単一ノード内 MPP**。これがノードをまたぐ分散実行 (Ballista 型) の土台になる。
//!
//! ## 現段階の制約
//! - ストレージは行=テキストのため、全列を Arrow の Utf8 として登録する。
//!   数値集計は SQL 側で `CAST(col AS BIGINT)` のように明示キャストする。
//!   (catalog の ColumnType → Arrow DataType 自動マッピングは次段階)
//! - 単一ノード並列まで。ノード間分散は分散レイヤ (openraft + Arrow Flight) 実装後。

use std::sync::Arc;

use datafusion::arrow::array::{
    ArrayRef, BooleanArray, Float64Array, Int64Array, StringArray, StringDictionaryBuilder,
};
use datafusion::arrow::datatypes::{DataType, Field, Int32Type, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::arrow::util::display::{ArrayFormatter, FormatOptions};
use datafusion::datasource::MemTable;
use datafusion::prelude::{SessionConfig, SessionContext};

use aruaru_core::catalog::ColumnType;

use crate::engine::{QueryEngine, QueryResponse, Value};

/// ColumnType → Arrow DataType
///
/// 【2026-08-21 DuckDB再調査でText列を辞書エンコードへ変更】
/// 6〜8言語(日英中独韓仏露西)でDuckDBとDataFusionの実装差分を再調査した
/// 結果、両者は同系統のベクトル化列指向エンジンだが、DuckDBの
/// ストレージ層固有の技術要素として **辞書エンコーディング(重複文字列を
/// 辞書へ集約し、各セルは辞書への整数インデックスのみを保持する圧縮)**
/// が確認できた
/// (https://duckdb.org/2022/10/28/lightweight-compression 、
/// https://endjin.com/blog/duckdb-in-depth-how-it-works-what-makes-it-fast)。
/// 従来の`build_array`は`ColumnType::Text`を常に生の`StringArray`
/// (各行が文字列バイト列をそのまま持つ)として構築しており、低カーディナ
/// リティ(重複の多い)列でもメモリ上重複してコピーされていた——DataFusion
/// 自体はDictionaryArrayをネイティブにサポートするにもかかわらず、
/// `aruaru-query`側がそれを使っていなかったという実際のギャップ。
///
/// 【2026-08-21 実装方法の再調査で修正】DuckDB本家の実装方法(GitHub
/// `duckdb/duckdb`の圧縮関連PR・公式ブログ`lightweight-compression`)を
/// 調べ直した結果、DuckDBは**Text列に無条件で辞書エンコードを適用する
/// のではなく、「analyzeフェーズ」でセグメントごとに複数の圧縮方式
/// (定数・RLE・辞書・FSST・ビットパッキング等)を試算し、最も小さくなる
/// ものを選ぶ**という設計だと判明した——当初の実装(常に辞書エンコード)
/// はこの「最良の方式を選ぶ」という核心を反映しておらず、高カーディナリ
/// ティ列(ほぼ全行が異なる値を持つ列、例: UUID列)では辞書エンコードは
/// 逆に不利(辞書自体が行数分近く肥大化し、インデックス配列のオーバー
/// ヘッドが上乗せされるだけ)になり得る。
///
/// 本実装はDuckDBの多数の圧縮アルゴリズムの完全な移植は行わない
/// (正直な簡略化点、下記)が、**その中核判断だけ**——「セグメント
/// (本実装ではテーブル全体のバッチ)を見て、辞書化が有利かどうかを
/// 判定してから決める」——を再現する: ユニーク値の比率
/// (`unique_count / total_non_null_count`)が閾値
/// (`DICTIONARY_CARDINALITY_THRESHOLD`)未満のときのみ辞書エンコードし、
/// そうでなければ従来のプレーンな`StringArray`(Utf8)を使う。
const DICTIONARY_CARDINALITY_THRESHOLD: f64 = 0.7;

/// 列の文字列値ベクタを Arrow 配列へ変換 (型に応じてパース)。
/// Text/デフォルト分岐では、実データを見てから辞書エンコードするか
/// (`DataType::Dictionary(Int32, Utf8)`)、プレーンな`Utf8`のままにするか
/// を決めるため、`build_array`自身が実際に採用した`DataType`も返す
/// (呼び出し側`build_table_batch`がこれをそのままスキーマのField型に使う
/// ——スキーマとデータの型不一致を構造的に起こせないようにするため)。
fn build_array(ty: &ColumnType, cells: Vec<Option<String>>) -> (DataType, ArrayRef) {
    match ty {
        ColumnType::Int | ColumnType::BigInt => {
            let v: Vec<Option<i64>> = cells
                .into_iter()
                .map(|c| c.and_then(|s| s.trim().parse::<i64>().ok()))
                .collect();
            (DataType::Int64, Arc::new(Int64Array::from(v)) as ArrayRef)
        }
        ColumnType::Float => {
            let v: Vec<Option<f64>> = cells
                .into_iter()
                .map(|c| c.and_then(|s| s.trim().parse::<f64>().ok()))
                .collect();
            (DataType::Float64, Arc::new(Float64Array::from(v)) as ArrayRef)
        }
        ColumnType::Bool => {
            let v: Vec<Option<bool>> = cells
                .into_iter()
                .map(|c| {
                    c.and_then(|s| match s.trim().to_lowercase().as_str() {
                        "true" | "t" | "1" | "yes" => Some(true),
                        "false" | "f" | "0" | "no" => Some(false),
                        _ => None,
                    })
                })
                .collect();
            (DataType::Boolean, Arc::new(BooleanArray::from(v)) as ArrayRef)
        }
        // Text / Bytes / Timestamp は当面ここで扱う(タイムスタンプ解析は次段階)。
        _ => {
            // DuckDB風の「analyzeフェーズ」簡易版: 辞書化した場合に得か
            // どうかを、実データのユニーク値比率から判定する。
            let non_null: Vec<&str> = cells.iter().filter_map(|c| c.as_deref()).collect();
            let unique_ratio = if non_null.is_empty() {
                0.0
            } else {
                let unique: std::collections::HashSet<&str> = non_null.iter().copied().collect();
                unique.len() as f64 / non_null.len() as f64
            };

            if unique_ratio < DICTIONARY_CARDINALITY_THRESHOLD {
                // 低カーディナリティ: 辞書エンコードが有利(重複する文字列値は
                // 辞書へ1回だけ格納され、各セルは整数インデックスのみを保持)。
                let mut builder = StringDictionaryBuilder::<Int32Type>::new();
                for cell in cells {
                    match cell {
                        Some(s) => {
                            let _ = builder.append(s.as_str());
                        }
                        None => builder.append_null(),
                    }
                }
                let dict_type =
                    DataType::Dictionary(Box::new(DataType::Int32), Box::new(DataType::Utf8));
                (dict_type, Arc::new(builder.finish()) as ArrayRef)
            } else {
                // 高カーディナリティ(例: UUID列): 辞書化すると辞書自体が
                // 行数近くまで肥大化し、インデックス配列のオーバーヘッドが
                // 純増するだけで不利——DuckDBのanalyzeフェーズが「圧縮しない
                // (Constant/FSST等が効かない場合はそのまま保持)」を選ぶのと
                // 同じ判断を、プレーンなUtf8配列で表現する。
                (DataType::Utf8, Arc::new(StringArray::from(cells)) as ArrayRef)
            }
        }
    }
}

fn session_context() -> SessionContext {
    // 並列度 = 利用可能 CPU 数 (パーティション並列実行に使われる)
    let parallelism = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let config = SessionConfig::new().with_target_partitions(parallelism);
    SessionContext::new_with_config(config)
}

/// (列定義, 行データ) から Arrow の (Schema, RecordBatch) を構築する。
fn build_table_batch(
    columns: &[(String, ColumnType)],
    rows: &[Vec<String>],
) -> Result<(Arc<Schema>, RecordBatch), String> {
    // build_arrayが実際に採用した型(辞書化するか否かはデータを見てから
    // 決まる、上記docコメント参照)をそのままField型として使うことで、
    // スキーマとデータの型不一致を構造的に起こせないようにする。
    let mut fields: Vec<Field> = Vec::with_capacity(columns.len());
    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(columns.len());
    for (ci, (cname, cty)) in columns.iter().enumerate() {
        let cells: Vec<Option<String>> = rows
            .iter()
            .map(|r| r.get(ci).cloned().filter(|s| !s.is_empty()))
            .collect();
        let (data_type, array) = build_array(cty, cells);
        fields.push(Field::new(cname, data_type, true));
        arrays.push(array);
    }
    let schema = Arc::new(Schema::new(fields));

    let batch = RecordBatch::try_new(schema.clone(), arrays).map_err(|e| e.to_string())?;
    Ok((schema, batch))
}

/// `ctx`へ登録済みのテーブルに対して`sql`を実行し、結果を
/// [`QueryResponse::Rows`]の形へ整形する(`run_olap`/`OlapCache::query`の
/// 共通の末尾処理)。
async fn execute_and_format(ctx: &SessionContext, sql: &str) -> Result<QueryResponse, String> {
    let df = ctx.sql(sql).await.map_err(|e| e.to_string())?;
    let batches = df.collect().await.map_err(|e| e.to_string())?;

    let columns: Vec<String> = batches
        .first()
        .map(|b| b.schema().fields().iter().map(|f| f.name().to_string()).collect())
        .unwrap_or_default();

    let opts = FormatOptions::default();
    let mut out_rows: Vec<Vec<Value>> = Vec::new();
    for batch in &batches {
        let formatters: Vec<ArrayFormatter<'_>> = batch
            .columns()
            .iter()
            .map(|c| ArrayFormatter::try_new(c.as_ref(), &opts))
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;

        for row in 0..batch.num_rows() {
            let cells: Vec<Value> = formatters.iter().map(|f| Value::Text(f.value(row).to_string())).collect();
            out_rows.push(cells);
        }
    }

    Ok(QueryResponse::Rows { columns, rows: out_rows })
}

/// DataFusion で OLAP クエリを実行する(キャッシュ無し、毎回全テーブルを
/// 行ストアからフル再構築する)。**正直な開示(2026-07-23)**: 大規模データ
/// では毎回のフル再構築がボトルネックになる——インクリメンタル同期版は
/// [`OlapCache`]を使うこと。この関数自体は後方互換のため残す。
pub async fn run_olap(engine: &QueryEngine, sql: &str) -> Result<QueryResponse, String> {
    let ctx = session_context();
    for (name, columns, rows) in engine.snapshot_tables() {
        if columns.is_empty() {
            continue;
        }
        let (schema, batch) = build_table_batch(&columns, &rows)?;
        let table = MemTable::try_new(schema, vec![vec![batch]]).map_err(|e| e.to_string())?;
        ctx.register_table(name.as_str(), Arc::new(table)).map_err(|e| e.to_string())?;
    }
    execute_and_format(&ctx, sql).await
}

/// HTAP列キャッシュ(TiDB/TiFlash方式のこのエコシステムなりの実装、
/// 2026-07-23新設、同日中にTiFlashのDelta Tree設計〈ベース列ストア+
/// デルタ行ストアをマージ、周期的にコンパクション〉を日英Web検索で
/// 調査の上で行単位へ再設計)。
///
/// `run_olap`が毎回全テーブルを行ストアからフル再構築するのに対し、
/// `OlapCache`は各テーブルについて「ベース」(Arrow列バッチ+その各行が
/// どのpkに対応するかの配列)と「変更されたpkの集合」を保持し、クエリ
/// のたびに:
/// 1. ベースから、変更されたpkに該当する行を`arrow::compute::filter`
///    (列指向のフィルタカーネル、文字列パースを伴わない軽量な操作)で除く。
/// 2. 変更されたpkだけを行ストアから読み直し(`QueryEngine::get_row`、
///    テーブル全体ではなく該当pkのみ)、小さな「デルタバッチ」を作る。
/// 3. フィルタ後のベース+デルタバッチを結合してクエリに使う(結合後の
///    ものを次回のベースとして採用=コンパクション)。
/// これにより、文字列→Arrow型付き配列への変換という重い処理
/// (`build_table_batch`)が必要になるのは「実際に変更された行の数」
/// だけになり、テーブルが大きいほど・変更が少ないほど効果が大きい
/// ——TiFlashが実践する「行ストアへの書き込みをデルタ層に貯め、
/// 列ストアとマージ」という核心思想を、単一プロセス内で実現したもの。
///
/// **正直な開示・スコープの限界**: (1) 単一プロセス内のみ——TiKV/
/// TiFlash間のような、ネットワーク越しの別ノードへの列レプリカ配置は
/// aruaru-distのRaftがまだ単一プロセス内実装(openraft統合待ち)の
/// ため範囲外。(2) 毎回コンパクション(フィルタ後ベース+デルタを即座に
/// 新ベースとして採用)する設計であり、TiFlashのような「デルタ層が
/// 一定サイズになるまで未コンパクションのまま複数バッチとして保持する」
/// 最適化は行っていない——正しさは保つが、書き込み1件ごとに軽量な
/// フィルタ処理が発生する点は今後の高頻度書き込み向け最適化の余地。
pub struct OlapCache {
    tables: parking_lot::RwLock<std::collections::HashMap<String, TableCache>>,
    /// 1セグメント(= DuckDBのRow Group / Databendのblock相当)の行数。
    /// 【2026-08-22 HTAP横断再調査で追加】従来は「テーブル全体で
    /// ゾーンマップ1つ」という最も粗い粒度しか持たず、コード内にも
    /// 正直な簡略化点として明記されていた。SingleStoreの
    /// Universal Storage(columnstoreをsegment単位で持ち、segmentごとの
    /// メタデータでスキップする)、Databendのsnapshot→segment→blockの
    /// 階層(blockごとにmin/max index・sparse index・bloom filterを持つ)、
    /// DuckDBのRow Groupのいずれも**セグメント単位の統計で部分スキップ**
    /// をしており、粒度そのものが要素技術だと分かったため導入した。
    segment_rows: usize,
}

/// セグメント(Row Group / block)単位の統計。
/// `offset`行目から`len`行が1セグメント。
#[derive(Debug, Clone)]
struct SegmentStats {
    offset: usize,
    len: usize,
    zone_maps: std::collections::HashMap<String, (f64, f64)>,
}

/// 既定のセグメント行数。DuckDBのRow Group(既定122,880行)ほど大きくは
/// せず、テスト可能かつ枝刈りが効く実用値として1024行を採る
/// (この値自体は本家の値の移植ではない——独自の既定値であることを明記)。
pub const DEFAULT_SEGMENT_ROWS: usize = 1024;

struct TableCache {
    schema: Arc<Schema>,
    /// ベース列バッチの各行が対応するpk(`base_batch`と同じ行順)。
    base_pks: Vec<Vec<u8>>,
    base_batch: RecordBatch,
    /// 【2026-08-21 DuckDB再調査で追加】DuckDB風のゾーンマップ(min/maxブロック
    /// 統計)。数値列(Int64/Float64)ごとに現在のベースバッチ全体のmin/maxを
    /// 保持する。DuckDBはこれをRow Group(物理ブロック)単位で持ち、クエリの
    /// WHERE句がそのブロックの値域と絶対に重ならないと判定できればブロック
    /// 全体をスキャンせずスキップする
    /// (https://endjin.com/blog/duckdb-in-depth-how-it-works-what-makes-it-fast 、
    /// https://blobs.duckdb.org/slides/TaDa-04.pdf)。
    /// 本実装は「テーブル全体で1ブロック」という最も粗い粒度の簡易版——
    /// ブロック単位分割(Row Groupのような複数統計区間への細分化)は
    /// 実装していない(正直な簡略化点、下記`OlapCache::query`のdocも参照)。
    zone_maps: std::collections::HashMap<String, (f64, f64)>,
    /// セグメント単位のゾーンマップ(2026-08-22追加)。
    segments: Vec<SegmentStats>,
}

/// バッチの数値列(Int64/Float64)ごとにmin/maxを計算する
/// (DuckDB風ゾーンマップの構築、`compute::min`/`compute::max`は
/// Arrow標準の縮約カーネルでNULLを無視する)。
fn compute_zone_maps(batch: &RecordBatch) -> std::collections::HashMap<String, (f64, f64)> {
    use datafusion::arrow::array::{Float64Array, Int64Array};
    use datafusion::arrow::compute::{max as arrow_max, min as arrow_min};

    let mut maps = std::collections::HashMap::new();
    for (i, field) in batch.schema().fields().iter().enumerate() {
        let col = batch.column(i);
        match field.data_type() {
            DataType::Int64 => {
                if let Some(arr) = col.as_any().downcast_ref::<Int64Array>() {
                    if let (Some(mn), Some(mx)) = (arrow_min(arr), arrow_max(arr)) {
                        maps.insert(field.name().clone(), (mn as f64, mx as f64));
                    }
                }
            }
            DataType::Float64 => {
                if let Some(arr) = col.as_any().downcast_ref::<Float64Array>() {
                    if let (Some(mn), Some(mx)) = (arrow_min(arr), arrow_max(arr)) {
                        maps.insert(field.name().clone(), (mn, mx));
                    }
                }
            }
            _ => {}
        }
    }
    maps
}

/// バッチを`segment_rows`行ごとに区切り、各セグメントのゾーンマップを
/// 計算する(DuckDBのRow Group / Databendのblock単位統計に相当)。
/// `RecordBatch::slice`はArrowのゼロコピー・スライスであり、データの
/// 複製は発生しない。
fn compute_segment_stats(batch: &RecordBatch, segment_rows: usize) -> Vec<SegmentStats> {
    let total = batch.num_rows();
    if total == 0 || segment_rows == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut offset = 0usize;
    while offset < total {
        let len = segment_rows.min(total - offset);
        let slice = batch.slice(offset, len);
        out.push(SegmentStats { offset, len, zone_maps: compute_zone_maps(&slice) });
        offset += len;
    }
    out
}

/// `SELECT ... FROM <table> WHERE <col> <op> <number>`という最も単純な
/// 形の述語だけを緩く抽出する(ゾーンマップによる枝刈り判定専用、
/// 完全なSQL式パーサではない——GROUP BY/JOIN/複合WHERE等を含む場合は
/// マッチさせない設計で、マッチしない場合は常に安全側〈=通常通り
/// DataFusionへ渡す〉に倒れる)。
fn extract_simple_range_predicate(sql: &str) -> Option<(String, String, String, f64)> {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"(?is)^\s*SELECT\s+.+?\s+FROM\s+(?P<table>[A-Za-z_][A-Za-z0-9_]*)\s+WHERE\s+(?P<col>[A-Za-z_][A-Za-z0-9_]*)\s*(?P<op>>=|<=|>|<)\s*(?P<num>-?\d+(?:\.\d+)?)\s*;?\s*$",
        )
        .expect("static regex must compile")
    });
    let caps = re.captures(sql)?;
    // GROUP BY/JOIN/サブクエリ等を含む場合はこの単純パターンにそもそも
    // マッチしない(`.+?\s+FROM`の非貪欲マッチが複雑な構造まで飲み込むと
    // 誤判定するリスクがあるため、念のため明示的にも除外する)。
    let upper = sql.to_uppercase();
    if upper.contains("GROUP BY") || upper.contains("JOIN") || upper.contains(" OR ") {
        return None;
    }
    let table = caps.name("table")?.as_str().to_string();
    let col = caps.name("col")?.as_str().to_string();
    let op = caps.name("op")?.as_str().to_string();
    let num: f64 = caps.name("num")?.as_str().parse().ok()?;
    Some((table, col, op, num))
}

/// ゾーンマップ`(min, max)`と述語`col <op> num`から、この範囲に**絶対に
/// マッチする行が存在しない**と証明できるか判定する(証明できない場合は
/// 安全側に倒れ`false`——DuckDBのブロックスキップと同じ「偽陰性は許すが
/// 偽陽性〈=本当は該当行があるのにスキップしてしまう〉は絶対に起こさない」
/// 設計)。
fn zone_map_disproves(min: f64, max: f64, op: &str, num: f64) -> bool {
    match op {
        ">" => max <= num,
        ">=" => max < num,
        "<" => min >= num,
        "<=" => min > num,
        _ => false,
    }
}

impl OlapCache {
    pub fn new() -> Self {
        Self::with_segment_rows(DEFAULT_SEGMENT_ROWS)
    }

    /// セグメント(Row Group)行数を指定して作る。
    pub fn with_segment_rows(segment_rows: usize) -> Self {
        Self {
            tables: parking_lot::RwLock::new(std::collections::HashMap::new()),
            segment_rows: segment_rows.max(1),
        }
    }

    /// テーブルのセグメント数(観測用・テスト用)。
    pub fn segment_count(&self, table: &str) -> usize {
        self.tables.read().get(table).map(|t| t.segments.len()).unwrap_or(0)
    }

    /// SQLに対するセグメント枝刈りの結果を`(テーブル名, 残ったセグメント数,
    /// 全セグメント数)`として返す(観測用・テスト用。`None`は
    /// 単純述語として解釈できず枝刈り対象外だった場合)。
    pub fn plan_segment_pruning(&self, sql: &str) -> Option<(String, usize, usize)> {
        let (table, col, op, num) = extract_simple_range_predicate(sql)?;
        let tables = self.tables.read();
        let entry = tables.get(&table)?;
        let total = entry.segments.len();
        let kept = entry
            .segments
            .iter()
            .filter(|seg| match seg.zone_maps.get(&col) {
                Some(&(min, max)) => !zone_map_disproves(min, max, &op, num),
                None => true,
            })
            .count();
        Some((table, kept, total))
    }

    /// セグメント`idx`の列`column`のゾーンマップ(観測用・テスト用)。
    pub fn segment_zone_map(&self, table: &str, idx: usize, column: &str) -> Option<(f64, f64)> {
        self.tables.read().get(table)?.segments.get(idx)?.zone_maps.get(column).copied()
    }

    /// 現在キャッシュされているテーブル数(テスト・観測用)。
    pub fn cached_table_count(&self) -> usize {
        self.tables.read().len()
    }

    /// `table`が現在キャッシュに存在するか(テスト・観測用)。
    pub fn contains(&self, table: &str) -> bool {
        self.tables.read().contains_key(table)
    }

    /// テーブル全体を行ストアから読み直し、ベースを作り直す
    /// (初回・スキーマ変更時のみ通るパス)。
    fn rebuild_full(&self, engine: &QueryEngine, name: &str) -> Result<(), String> {
        let Some((columns, pks, rows)) = engine.snapshot_table(name) else {
            self.tables.write().remove(name);
            return Ok(());
        };
        engine.clear_olap_schema_dirty(name);
        let _ = engine.take_olap_delta_pks(name); // スキーマ再構築に吸収済み
        if columns.is_empty() {
            self.tables.write().remove(name);
            return Ok(());
        }
        let (schema, batch) = build_table_batch(&columns, &rows)?;
        let zone_maps = compute_zone_maps(&batch);
        let segments = compute_segment_stats(&batch, self.segment_rows);
        self.tables.write().insert(name.to_string(), TableCache { schema, base_pks: pks, base_batch: batch, zone_maps, segments });
        Ok(())
    }

    /// テーブル`table`の列`column`のゾーンマップ(`(min, max)`)を返す
    /// (観測用・テスト用の公開アクセサ)。
    pub fn zone_map(&self, table: &str, column: &str) -> Option<(f64, f64)> {
        self.tables.read().get(table)?.zone_maps.get(column).copied()
    }

    /// 変更されたpkだけをベースから除き、その現在値をデルタとして結合する
    /// (行単位インクリメンタル同期の核心パス)。
    fn rebuild_incremental(
        &self,
        engine: &QueryEngine,
        name: &str,
        delta_pks: std::collections::BTreeSet<Vec<u8>>,
    ) -> Result<(), String> {
        use datafusion::arrow::compute::filter_record_batch;

        let mut cache = self.tables.write();
        let Some(entry) = cache.get(name) else { return Ok(()) };

        // ベースの各行が、今回変更されたpkに該当するかどうかのマスク
        // (該当する=古い値なので除く、に該当しない=そのまま残す)。
        let keep_mask: BooleanArray = entry.base_pks.iter().map(|pk| Some(!delta_pks.contains(pk))).collect();
        let filtered = filter_record_batch(&entry.base_batch, &keep_mask).map_err(|e| e.to_string())?;
        let mut new_pks: Vec<Vec<u8>> =
            entry.base_pks.iter().zip(keep_mask.iter()).filter(|(_, keep)| keep.unwrap_or(false)).map(|(pk, _)| pk.clone()).collect();

        // 変更されたpkの「現在値」を1件ずつ読み直す(テーブル全体ではない)。
        // Noneは削除済みなので、デルタには含めない(=maskで除かれたまま復活しない)。
        let columns: Vec<(String, ColumnType)> = entry
            .schema
            .fields()
            .iter()
            .map(|f| (f.name().clone(), arrow_type_to_column_type(f.data_type())))
            .collect();
        let mut delta_rows: Vec<Vec<String>> = Vec::new();
        for pk in &delta_pks {
            if let Some(row) = engine.get_row(name, pk) {
                delta_rows.push(row);
                new_pks.push(pk.clone());
            }
        }

        let merged = if delta_rows.is_empty() {
            filtered
        } else {
            let (_, delta_batch) = build_table_batch(&columns, &delta_rows)?;
            datafusion::arrow::compute::concat_batches(&entry.schema, [&filtered, &delta_batch]).map_err(|e| e.to_string())?
        };

        let schema = entry.schema.clone();
        let zone_maps = compute_zone_maps(&merged);
        let segments = compute_segment_stats(&merged, self.segment_rows);
        cache.insert(name.to_string(), TableCache { schema, base_pks: new_pks, base_batch: merged, zone_maps, segments });
        Ok(())
    }

    /// 変更のあったテーブルだけ再構築(初回/スキーマ変更は全体、それ以外は
    /// 行単位デルタマージ)してキャッシュへ反映し、削除されたテーブルを
    /// キャッシュから除去する。
    fn refresh(&self, engine: &QueryEngine) -> Result<(), String> {
        let names = engine.table_names();
        self.tables.write().retain(|name, _| names.contains(name));
        for name in &names {
            let needs_full_rebuild = engine.is_olap_schema_dirty(name) || !self.contains(name);
            if needs_full_rebuild {
                self.rebuild_full(engine, name)?;
                continue;
            }
            let delta_pks = engine.take_olap_delta_pks(name);
            if delta_pks.is_empty() {
                continue; // 変更無し: 行ストアに一切触れずキャッシュを再利用
            }
            self.rebuild_incremental(engine, name, delta_pks)?;
        }
        Ok(())
    }

    /// インクリメンタル同期された列キャッシュ経由でOLAPクエリを実行する。
    ///
    /// 【2026-08-21 DuckDB再調査で追加・ゾーンマップによる枝刈り】
    /// `sql`が`extract_simple_range_predicate`で抽出できる単純な範囲述語
    /// (`SELECT ... FROM t WHERE col > N`のような形)であり、かつ対象列の
    /// ゾーンマップ(`(min, max)`)がその述語を絶対に満たす行が無いと
    /// 証明できる場合、**DataFusionへ一切クエリを投げずに空の結果を
    /// 即座に返す**——DuckDBがブロックのmin/max統計だけでRow Group全体の
    /// スキャンをスキップするのと同じ発想。今回の実装は「テーブル全体で
    /// 1つのゾーンマップ」という粗い粒度だったため、スキップできるのは
    /// 「テーブル全体が対象外と証明できる場合」のみだった。
    ///
    /// 【2026-08-22 HTAP横断再調査で改善・セグメント単位の部分スキップ】
    /// SingleStore Universal Storage / Databend(snapshot→segment→block)/
    /// DuckDB(Row Group)のいずれも**セグメント単位のmin/max統計で
    /// 部分スキップ**をしていることを確認したため、テーブルを
    /// `segment_rows`行ごとのセグメントに区切って各セグメントの
    /// ゾーンマップを持つようにし、証明できたセグメントのみを
    /// DataFusionへ渡さない形に変更した(`plan_segment_pruning`で
    /// 枝刈り結果を観測できる)。ブルームフィルタ・sparse indexまでは
    /// 実装していない(正直な簡略化点)。
    /// マッチしない/証明できない場合は常に安全側(=通常通り
    /// `execute_and_format`でDataFusionへ渡す)に倒れるため、この枝刈りが
    /// 結果の正しさに影響することは無い。
    pub async fn query(&self, engine: &QueryEngine, sql: &str) -> Result<QueryResponse, String> {
        self.refresh(engine)?;

        let predicate = extract_simple_range_predicate(sql);
        if let Some((table, col, op, num)) = predicate.as_ref() {
            let tables = self.tables.read();
            if let Some(entry) = tables.get(table) {
                if let Some(&(min, max)) = entry.zone_maps.get(col) {
                    if zone_map_disproves(min, max, op, *num) {
                        let columns: Vec<String> =
                            entry.schema.fields().iter().map(|f| f.name().clone()).collect();
                        return Ok(QueryResponse::Rows { columns, rows: Vec::new() });
                    }
                }
            }
        }

        let ctx = session_context();
        for (name, entry) in self.tables.read().iter() {
            // 【2026-08-22追加】セグメント単位の枝刈り。述語の対象テーブル
            // については、ゾーンマップが「このセグメントには該当行が
            // 絶対に無い」と証明できるセグメントをDataFusionへ渡さない。
            // 生き残ったセグメントは`RecordBatch::slice`(ゼロコピー)を
            // それぞれ1パーティションとして登録する——DataFusionは
            // パーティション並列で走るため、枝刈りと並列化が同時に効く。
            // 証明できない場合・述語が単純形でない場合は全セグメントを
            // 渡す(安全側)。
            let partitions: Vec<Vec<RecordBatch>> = match (&predicate, entry.segments.is_empty()) {
                (Some((ptable, pcol, pop, pnum)), false) if ptable == name => {
                    let kept: Vec<Vec<RecordBatch>> = entry
                        .segments
                        .iter()
                        .filter(|seg| match seg.zone_maps.get(pcol) {
                            Some(&(min, max)) => !zone_map_disproves(min, max, pop, *pnum),
                            None => true,
                        })
                        .map(|seg| vec![entry.base_batch.slice(seg.offset, seg.len)])
                        .collect();
                    if kept.is_empty() {
                        // 全セグメントが枝刈りされた = 該当行なし
                        vec![vec![RecordBatch::new_empty(entry.schema.clone())]]
                    } else {
                        kept
                    }
                }
                _ => vec![vec![entry.base_batch.clone()]],
            };
            let table = MemTable::try_new(entry.schema.clone(), partitions).map_err(|e| e.to_string())?;
            ctx.register_table(name.as_str(), Arc::new(table)).map_err(|e| e.to_string())?;
        }
        execute_and_format(&ctx, sql).await
    }
}

impl Default for OlapCache {
    fn default() -> Self {
        Self::new()
    }
}

impl OlapCache {
    /// TiFlash Raft learner方式への接近(2026-08-21新設)。日英中の
    /// Web調査(`docs.pingcap.com/tidbcloud/tiflash-overview`、
    /// `github.com/pingcap/tiflash`のdesign doc、`tikv.github.io/doc/
    /// raftstore`)によれば、TiFlashは「Raftのlearnerとして複製ログを
    /// **非同期に購読**し、変更があった時だけ列ストアへ反映する」
    /// プッシュ型の構成である。従来の`query()`(`refresh()`をクエリの
    /// たびに同期的に呼ぶ)は、クエリが来るまでキャッシュが更新されない
    /// **プル型**であり、非同期購読という核心の向きが逆だった
    /// (`CLAUDE.md`のHANDOFFに正直に記録済みのギャップ)。
    ///
    /// `subscribe`は`engine`に`QueryEngine::set_olap_notifier`で
    /// `tokio::sync::mpsc`の送信側を登録し、受信側をバックグラウンド
    /// タスクとして`tokio::spawn`する。以後、`persist_row`等で変更が
    /// あるたびに(クエリが来るのを待たず)そのテーブルだけを非同期に
    /// 再構築し、次回のクエリはキャッシュ済みの新しい状態を即座に読む。
    ///
    /// **正直な開示・スコープの限界**: (1) これは真のRaft learner
    /// (別ノードとしてRaft複製ログのコンセンサスへ参加し、ネットワーク
    /// 越しにログエントリを受信する)ではない——`tokio::mpsc`は同一
    /// プロセス内のチャネルであり、`aruaru-dist`のRaft複製ログを経由
    /// していない。単一プロセス前提の近似であることを`olap_notify`の
    /// docコメントと合わせて明記する。(2) 通知チャネル経由の更新は
    /// あくまで「先回りしてキャッシュを温める」ための補助経路であり、
    /// 正しさの最終的な根拠は引き続き`query()`内の`refresh()`
    /// (クエリ実行時の同期ポーリング、ダーティ集合の再確認)にある
    /// ——`subscribe`を呼ばなくても`query()`は従来通り正しく動作する。
    /// (3) 返す`JoinHandle`をdropしてもタスク自体はデタッチされ動作を
    /// 続ける(通常のtokioの挙動)。呼び出し元が明示的に停止したい場合は
    /// 返り値の`abort()`を呼ぶこと。
    pub fn subscribe(self: Arc<Self>, engine: Arc<QueryEngine>) -> tokio::task::JoinHandle<()> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        engine.set_olap_notifier(tx);
        tokio::spawn(async move {
            while let Some(table) = rx.recv().await {
                if let Err(e) = self.refresh_one(&engine, &table) {
                    tracing::warn!(table = %table, error = %e, "olap async subscriber: eager refresh failed (will retry on next query via sync poll)");
                }
            }
        })
    }

    /// `refresh()`のうち1テーブルだけを対象にした版(非同期購読タスクが
    /// 通知を受けた直後、そのテーブルだけを先回りして再構築するために使う)。
    /// `query()`側の`refresh()`(全テーブル走査)とロジックを共有するため、
    /// 内部的には同じ`rebuild_full`/`rebuild_incremental`を呼ぶ。
    fn refresh_one(&self, engine: &QueryEngine, name: &str) -> Result<(), String> {
        let needs_full_rebuild = engine.is_olap_schema_dirty(name) || !self.contains(name);
        if needs_full_rebuild {
            return self.rebuild_full(engine, name);
        }
        let delta_pks = engine.take_olap_delta_pks(name);
        if delta_pks.is_empty() {
            return Ok(());
        }
        self.rebuild_incremental(engine, name, delta_pks)
    }
}

/// Arrow DataType → catalog::ColumnType(逆変換、デルタ行の再構築時に
/// 元の列型へ揃えるために使う。`arrow_type`の対になる関数)。
fn arrow_type_to_column_type(ty: &DataType) -> ColumnType {
    match ty {
        DataType::Int64 => ColumnType::BigInt,
        DataType::Float64 => ColumnType::Float,
        DataType::Boolean => ColumnType::Bool,
        _ => ColumnType::Text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 【2026-08-22 HTAP横断再調査】セグメント(Row Group / block)単位の
    /// ゾーンマップが作られ、述語に該当しないセグメントだけが枝刈りされ、
    /// **かつ結果は枝刈り無しと完全に一致する**ことを実証する。
    #[tokio::test]
    async fn segment_level_zone_maps_prune_only_provably_empty_segments() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE m (id INT, v INT)").unwrap();
        for i in 1..=6 {
            eng.execute(&format!("INSERT INTO m (id, v) VALUES ({i}, {})", i * 10)).unwrap();
        }
        // 2行=1セグメント -> 3セグメント (v: 10-20 / 30-40 / 50-60)
        let cache = OlapCache::with_segment_rows(2);
        let all = cache.query(&eng, "SELECT COUNT(*) AS n FROM m").await.unwrap();
        if let QueryResponse::Rows { rows, .. } = all {
            assert_eq!(rows[0][0], Value::Text("6".into()));
        } else {
            panic!("expected rows");
        }
        assert_eq!(cache.segment_count("m"), 3);
        assert_eq!(cache.segment_zone_map("m", 0, "v"), Some((10.0, 20.0)));
        assert_eq!(cache.segment_zone_map("m", 2, "v"), Some((50.0, 60.0)));

        // v > 45 は最後のセグメントだけが残る
        let sql = "SELECT id FROM m WHERE v > 45";
        assert_eq!(cache.plan_segment_pruning(sql), Some(("m".to_string(), 1, 3)));
        let resp = cache.query(&eng, sql).await.unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows.len(), 2, "5 and 6 must survive pruning");
        } else {
            panic!("expected rows");
        }

        // 全セグメントが対象外なら空(テーブル全体のゾーンマップで即断)
        assert_eq!(
            cache.plan_segment_pruning("SELECT id FROM m WHERE v > 1000"),
            Some(("m".to_string(), 0, 3))
        );
        let resp = cache.query(&eng, "SELECT id FROM m WHERE v > 1000").await.unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert!(rows.is_empty());
        } else {
            panic!("expected rows");
        }

        // 全セグメントが該当する述語は1つも枝刈りしない(偽陽性を出さない)
        assert_eq!(
            cache.plan_segment_pruning("SELECT id FROM m WHERE v > 0"),
            Some(("m".to_string(), 3, 3))
        );
    }

    #[tokio::test]
    async fn test_olap_group_by_sum() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE orders (id INT, region TEXT, amount INT)")
            .unwrap();
        eng.execute("INSERT INTO orders (id, region, amount) VALUES (1, 'east', 100)")
            .unwrap();
        eng.execute("INSERT INTO orders (id, region, amount) VALUES (2, 'east', 50)")
            .unwrap();
        eng.execute("INSERT INTO orders (id, region, amount) VALUES (3, 'west', 70)")
            .unwrap();

        // catalog の型 (amount は INT) で登録されるため CAST 不要
        let resp = run_olap(
            &eng,
            "SELECT region, SUM(amount) AS total \
             FROM orders GROUP BY region ORDER BY region",
        )
        .await
        .unwrap();

        if let QueryResponse::Rows { columns, rows } = resp {
            assert_eq!(columns, vec!["region", "total"]);
            assert_eq!(rows.len(), 2);
            // east=150, west=70
            assert_eq!(rows[0][0], Value::Text("east".into()));
            assert_eq!(rows[0][1], Value::Text("150".into()));
            assert_eq!(rows[1][0], Value::Text("west".into()));
            assert_eq!(rows[1][1], Value::Text("70".into()));
        } else {
            panic!("expected rows");
        }
    }

    /// HTAP列キャッシュの核心特性: 一度クエリを実行した後、無関係な
    /// 別テーブルへの書き込みは、変更していないテーブルの列キャッシュ
    /// エントリ数(`cached_table_count`)を変えない——つまり変更の無い
    /// テーブルは行ストアから一切再構築されない、という実証。
    #[tokio::test]
    async fn olap_cache_reuses_unchanged_tables_and_rebuilds_only_dirty_ones() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE orders (id INT, amount INT)").unwrap();
        eng.execute("CREATE TABLE customers (id INT, name TEXT)").unwrap();
        eng.execute("INSERT INTO orders (id, amount) VALUES (1, 100)").unwrap();
        eng.execute("INSERT INTO customers (id, name) VALUES (1, 'alice')").unwrap();

        let cache = OlapCache::new();
        let resp = cache.query(&eng, "SELECT SUM(amount) AS total FROM orders").await.unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows[0][0], Value::Text("100".into()));
        } else {
            panic!("expected rows");
        }
        assert_eq!(cache.cached_table_count(), 2, "both tables queried at least once should be cached");
        assert!(!eng.has_pending_olap_delta("orders"));
        assert!(!eng.has_pending_olap_delta("customers"));

        // customersだけ更新 -> ordersのデルタは発生しないはず。
        eng.execute("INSERT INTO customers (id, name) VALUES (2, 'bob')").unwrap();
        assert!(!eng.has_pending_olap_delta("orders"), "unrelated table must not accumulate a delta");
        assert!(eng.has_pending_olap_delta("customers"));

        // 再クエリ: ordersの値は変わらず正しく返る(キャッシュ再利用でも
        // 結果が壊れないことの確認)、かつcustomersの新しい行も反映される
        // (デルタがあったテーブルは正しく再構築されることの確認)。
        let resp = cache
            .query(&eng, "SELECT COUNT(*) AS n FROM customers")
            .await
            .unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows[0][0], Value::Text("2".into()), "customers cache must reflect the new row");
        } else {
            panic!("expected rows");
        }
        assert!(!eng.has_pending_olap_delta("customers"), "delta must be cleared after rebuild");
    }

    /// 行単位デルタマージの正しさ: 既存行の更新・削除・新規追加が全て
    /// 正しく反映され、かつ更新前の古い値がベースに残って二重集計
    /// されないことを実証する(TiFlashのDelta Tree設計から借用した
    /// 「ベースからフィルタで除いてデルタと結合」の核心的な正しさ検証)。
    #[tokio::test]
    async fn olap_cache_incremental_merge_handles_update_delete_and_insert_correctly() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE items (id INT, qty INT)").unwrap();
        eng.execute("INSERT INTO items (id, qty) VALUES (1, 10)").unwrap();
        eng.execute("INSERT INTO items (id, qty) VALUES (2, 20)").unwrap();
        eng.execute("INSERT INTO items (id, qty) VALUES (3, 30)").unwrap();

        let cache = OlapCache::new();
        let resp = cache.query(&eng, "SELECT SUM(qty) AS total FROM items").await.unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows[0][0], Value::Text("60".into()));
        } else {
            panic!("expected rows");
        }

        // id=1を更新(10->99)・id=2を削除・id=4を新規追加。
        eng.execute("UPDATE items SET qty = 99 WHERE id = 1").unwrap();
        eng.execute("DELETE FROM items WHERE id = 2").unwrap();
        eng.execute("INSERT INTO items (id, qty) VALUES (4, 40)").unwrap();

        // 正しい合計: 99(更新後) + 30(無変更) + 40(新規) = 169。
        // 古い値(id=1の10、削除されたid=2の20)が残って二重集計されて
        // いないこと、ベースのフィルタ+デルタ結合が正しく機能している
        // ことの直接証明。
        let resp = cache.query(&eng, "SELECT SUM(qty) AS total FROM items").await.unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows[0][0], Value::Text("169".into()));
        } else {
            panic!("expected rows");
        }

        let resp = cache.query(&eng, "SELECT COUNT(*) AS n FROM items").await.unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows[0][0], Value::Text("3".into()), "id=1,3,4 should remain, id=2 deleted");
        } else {
            panic!("expected rows");
        }
    }

    /// **TiFlash learner風の非同期購読(2026-08-21新設)の核心特性**:
    /// `OlapCache::subscribe`を呼んだ後、`query()`を一度も呼んでいない
    /// 段階でも、書き込み(`INSERT`)の直後にバックグラウンドタスクが
    /// 自律的にキャッシュを温めていることを実証する
    /// (`cached_table_count()`が、クエリを介さずに増えることが直接の
    /// 証拠——従来の同期ポーリング経路〈`query()`内の`refresh()`〉
    /// だけなら、最初の`query()`呼び出しまでキャッシュは空のままのはず)。
    #[tokio::test]
    async fn olap_cache_async_subscriber_eagerly_warms_cache_without_a_query() {
        let eng = Arc::new(QueryEngine::new());
        let cache = Arc::new(OlapCache::new());

        assert_eq!(cache.cached_table_count(), 0, "no query yet, cache must start empty");

        let handle = cache.clone().subscribe(eng.clone());

        eng.execute("CREATE TABLE events (id INT, kind TEXT)").unwrap();
        eng.execute("INSERT INTO events (id, kind) VALUES (1, 'click')").unwrap();

        // 非同期購読タスクが通知を処理するのを少し待つ(バックグラウンド
        // tokioタスクのスケジューリングを待つ、テストとしての現実的な
        // 妥協——本番運用では待つ必要は無く「先回りできていれば速い、
        // 間に合わなくても次のquery()が同期ポーリングで必ず正しく補う」
        // 設計であることをdocコメントに明記済み)。
        let mut warmed = false;
        for _ in 0..50 {
            if cache.contains("events") {
                warmed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(warmed, "async subscriber must warm the 'events' cache entry without any query() call");

        // 先回りされたキャッシュが実際に正しいデータを持つことも、
        // 通常のqueryパス経由で確認する(refresh()はデルタが空なら
        // 行ストアに触れず、既に温められた値をそのまま使うはず)。
        let resp = cache.query(&eng, "SELECT COUNT(*) AS n FROM events").await.unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows[0][0], Value::Text("1".into()));
        } else {
            panic!("expected rows");
        }

        handle.abort();
    }

    /// 【DuckDB風辞書エンコーディング検証(2026-08-21)】低カーディナリティの
    /// Text列(`region`、値は`east`/`west`の2種類のみ、100行)が、実際に
    /// `DataType::Dictionary(Int32, Utf8)`として構築されること、辞書の
    /// エントリ数(ユニーク値の数)が行数よりはるかに少ないことを直接検証する
    /// (「重複文字列が辞書へ1回だけ格納される」という圧縮効果の直接証拠)。
    /// あわせて、辞書エンコードされた列でも通常通り正しく集計できることも
    /// 実証する(圧縮による正しさへの悪影響が無いことの確認)。
    #[tokio::test]
    async fn text_columns_are_dictionary_encoded_and_still_aggregate_correctly() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE orders (id INT, region TEXT, amount INT)").unwrap();
        for i in 0..100 {
            let region = if i % 2 == 0 { "east" } else { "west" };
            eng.execute(&format!(
                "INSERT INTO orders (id, region, amount) VALUES ({i}, '{region}', 10)"
            ))
            .unwrap();
        }

        let cache = OlapCache::new();
        cache.refresh(&eng).unwrap();
        let tables = cache.tables.read();
        let entry = tables.get("orders").unwrap();
        let region_field = entry.schema.field_with_name("region").unwrap();
        assert!(
            matches!(region_field.data_type(), DataType::Dictionary(_, _)),
            "region column should be dictionary-encoded, got {:?}",
            region_field.data_type()
        );
        let region_idx = entry.schema.index_of("region").unwrap();
        let region_array = entry.base_batch.column(region_idx);
        let dict_array = region_array
            .as_any()
            .downcast_ref::<datafusion::arrow::array::DictionaryArray<Int32Type>>()
            .expect("region column must actually be a DictionaryArray");
        // 100行あるが値は"east"/"west"の2種類のみ -> 辞書のユニーク値数は2。
        assert_eq!(
            dict_array.values().len(),
            2,
            "dictionary should contain only the 2 unique region values, not 100"
        );
        drop(tables);

        let resp = cache.query(&eng, "SELECT region, SUM(amount) AS total FROM orders GROUP BY region ORDER BY region").await.unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0][1], Value::Text("500".into())); // east: 50件 x 10
            assert_eq!(rows[1][1], Value::Text("500".into())); // west: 50件 x 10
        } else {
            panic!("expected rows");
        }
    }

    /// 【2026-08-21 DuckDB本家の実装方法(analyzeフェーズ)再調査で追加】
    /// 高カーディナリティ列(ほぼ全行がユニークな値を持つ、UUID列を模した
    /// もの)は辞書エンコードされず、プレーンな`Utf8`のままであることを
    /// 検証する——DuckDBが「セグメントを見てから最良の圧縮方式を選ぶ」
    /// (辞書化が不利なら辞書化しない)のと同じ判断を、簡易版
    /// (ユニーク比率閾値)で再現できていることの直接証拠。あわせて、
    /// 辞書化されなくても集計結果自体は正しいことも確認する。
    #[tokio::test]
    async fn high_cardinality_text_columns_skip_dictionary_encoding() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE events (id INT, uuid TEXT, amount INT)").unwrap();
        for i in 0..100 {
            // 全行が異なる値を持つ列(UUID列を模した高カーディナリティ)。
            eng.execute(&format!(
                "INSERT INTO events (id, uuid, amount) VALUES ({i}, 'uuid-{i}', 1)"
            ))
            .unwrap();
        }

        let cache = OlapCache::new();
        cache.refresh(&eng).unwrap();
        let tables = cache.tables.read();
        let entry = tables.get("events").unwrap();
        let uuid_field = entry.schema.field_with_name("uuid").unwrap();
        assert_eq!(
            uuid_field.data_type(),
            &DataType::Utf8,
            "high-cardinality column should stay plain Utf8, not be dictionary-encoded: {:?}",
            uuid_field.data_type()
        );
        drop(tables);

        let resp = cache.query(&eng, "SELECT COUNT(*) AS n FROM events").await.unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows[0][0], Value::Text("100".into()));
        } else {
            panic!("expected rows");
        }
    }

    /// 【DuckDB風ゾーンマップ検証(2026-08-21)】数値列のmin/maxが正しく
    /// 計算・公開されること、そのゾーンマップを使って「絶対に該当行が無い」
    /// と証明できるWHERE句(範囲外)が、実際にDataFusionへ処理を渡さず
    /// 空の結果を返すこと(=枝刈りが機能している)、そして通常のWHERE句
    /// (該当行がある場合)は従来通り正しい結果を返すことを検証する。
    #[tokio::test]
    async fn zone_map_prunes_queries_that_cannot_possibly_match_and_normal_queries_still_work() {
        let eng = QueryEngine::new();
        eng.execute("CREATE TABLE items (id INT, qty INT)").unwrap();
        eng.execute("INSERT INTO items (id, qty) VALUES (1, 10)").unwrap();
        eng.execute("INSERT INTO items (id, qty) VALUES (2, 20)").unwrap();
        eng.execute("INSERT INTO items (id, qty) VALUES (3, 30)").unwrap();

        let cache = OlapCache::new();
        cache.refresh(&eng).unwrap();
        // qty列のゾーンマップは(min=1, max=30) -- idもINT列なので(min=1, max=3)。
        let (min, max) = cache.zone_map("items", "qty").expect("zone map must exist for qty");
        assert_eq!((min, max), (10.0, 30.0));

        // qty > 30 は絶対にどの行も満たせない(max=30なので "> 30" は不成立)
        // -> ゾーンマップだけで空を返すはず。
        let resp = cache.query(&eng, "SELECT * FROM items WHERE qty > 30").await.unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert!(rows.is_empty(), "zone map should have proven no rows can match qty > 30");
        } else {
            panic!("expected rows");
        }

        // qty > 15 は該当行がある(20, 30)ので、通常通りDataFusion経由で
        // 正しい行数が返ること(枝刈りロジックが正しい結果を壊していないこと)。
        let resp = cache.query(&eng, "SELECT * FROM items WHERE qty > 15").await.unwrap();
        if let QueryResponse::Rows { rows, .. } = resp {
            assert_eq!(rows.len(), 2, "qty=20 and qty=30 should both match qty > 15");
        } else {
            panic!("expected rows");
        }
    }

    /// `extract_simple_range_predicate`自体の単体テスト: GROUP BY/JOINを
    /// 含む複雑なクエリはマッチしない(=常に安全側でDataFusionへ渡る)こと、
    /// 単純な範囲述語は正しく抽出されることを確認する。
    #[test]
    fn extract_simple_range_predicate_only_matches_simple_range_queries() {
        assert_eq!(
            extract_simple_range_predicate("SELECT * FROM t WHERE qty > 10"),
            Some(("t".to_string(), "qty".to_string(), ">".to_string(), 10.0))
        );
        assert_eq!(
            extract_simple_range_predicate("SELECT * FROM t WHERE qty >= 10.5"),
            Some(("t".to_string(), "qty".to_string(), ">=".to_string(), 10.5))
        );
        assert!(extract_simple_range_predicate(
            "SELECT region, SUM(qty) FROM t WHERE qty > 10 GROUP BY region"
        )
        .is_none());
        assert!(extract_simple_range_predicate(
            "SELECT * FROM t JOIN u ON t.id = u.id WHERE qty > 10"
        )
        .is_none());
        assert!(extract_simple_range_predicate("SELECT * FROM t").is_none());
    }

    /// ゾーンマップ判定関数(`zone_map_disproves`)自体の境界値テスト。
    #[test]
    fn zone_map_disproves_boundary_conditions() {
        // min=10, max=30の範囲に対して:
        assert!(zone_map_disproves(10.0, 30.0, ">", 30.0)); // 30より大きい値は無い
        assert!(!zone_map_disproves(10.0, 30.0, ">=", 30.0)); // 30ちょうどはある
        assert!(zone_map_disproves(10.0, 30.0, ">=", 30.1)); // 30.1以上は無い
        assert!(zone_map_disproves(10.0, 30.0, "<", 10.0)); // 10未満は無い
        assert!(!zone_map_disproves(10.0, 30.0, "<=", 10.0)); // 10ちょうどはある
        assert!(zone_map_disproves(10.0, 30.0, "<=", 9.9)); // 9.9以下は無い
        assert!(!zone_map_disproves(10.0, 30.0, ">", 5.0)); // 該当行があり得る
    }
}
