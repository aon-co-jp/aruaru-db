//! オブジェクトストレージ直結のテーブルフォーマット (Databend 方式)
//!
//! **背景 (2026-08-22)**: 「Snowflake と CockroachDB の両方の特性を持つ
//! ハイブリッドDB」に関する多言語 (日本語・英語・中国語・韓国語) 調査で、
//! **オブジェクトストレージを一次ストレージとして使う場合のメタデータ
//! 管理 (snapshot / segment / block の3層 + 統計索引 + 原子的コミット)**
//! が共通の要素技術だと分かったため実装したもの。
//!
//! ## 調査で確認した Databend の構造 (中国語一次資料)
//!
//! Databend 公式ブログ「Databend 存储架构总览」
//! (<https://www.cnblogs.com/databend/p/16814420.html>、
//! <https://zhuanlan.zhihu.com/p/575954636>) および「Databend 索引结构说明」
//! (<https://zhuanlan.zhihu.com/p/591205648>) によれば:
//!
//! - **snapshot**: 「相当于每一个数据的一个版本号」——書き込みのたびに
//!   新しいスナップショット (JSON) が生成され、内部に対応する segment
//!   ファイルの一覧を持つ。パスは
//!   `/bucket/[root]/<db_id>/<table_id>/_ss/<32桁16進>_v1.json`。
//! - **segment**: block をまとめる JSON ファイル。パスは
//!   `.../_sg/<32桁16進>_v1.json`。**1 snapshot の下に複数 segment、
//!   1 segment に最低1・最大1000 block**。
//! - **索引**: min/max index、sparse index、bloom filter index の3種を
//!   持ち、min/max と sparse index は block の parquet と segment の
//!   両方に格納される。
//! - **ACID**: 書き込みごとに新スナップショットを作り、**MetaSrv 上の
//!   そのテーブルの Snapshot Key の書き込みが成功して初めてコミット成功**
//!   と見なす (「MetaSrv 正是 Databend 实现 ACID 的基础」)。
//!
//! これは Snowflake の「不変マイクロパーティション + メタデータ層」や
//! Apache Iceberg の「snapshot → manifest list → manifest → data file」と
//! 同型であり、**計算ノードを増やしてもストレージを共有できる**
//! (= Snowflake 型のストレージ/コンピュート分離) 根拠になっている。
//!
//! ## コードで裏取りしたギャップ
//!
//! `aruaru-backup` には `s3.rs` (SigV4 presigned URL でのオブジェクト
//! PUT/GET/DELETE/LIST) は既にあったが、**「オブジェクトストレージ上の
//! ファイル群を1つのテーブルとして扱うためのメタデータ階層」が無かった**
//! ——バックアップ先として書き出すことはできても、その上で
//! 「スナップショット単位の一貫した読み取り」「統計によるブロック枝刈り」
//! 「原子的コミット」ができる形式ではなかった。本モジュールがそれを補う。
//!
//! ## スコープと正直な簡略化点 (誇張しない)
//!
//! 1. `ObjectStore` トレイト越しの抽象であり、**同梱の実装は
//!    メモリ上の `InMemoryObjectStore` のみ**。既存の
//!    [`crate::s3::S3Client`] への接続 (非同期 I/O) は未実装——
//!    次回課題として `CLAUDE.md` に記録する。
//! 2. block の実体 (Parquet ファイル) の書き出しは行わない。本モジュールは
//!    **メタデータ階層と枝刈り・コミットの正しさ**を担当し、`BlockMeta` は
//!    実体ファイルの位置と統計だけを持つ。
//! 3. 索引は min/max と bloom filter の2種。sparse index は未実装。
//! 4. bloom filter は sha256 から2つのハッシュを取り出す簡易版
//!    (偽陽性は許すが**偽陰性は起こさない**)。
//! 5. コミット衝突は「期待するスナップショットIDと一致しなければ失敗」
//!    という楽観的CAS 1回のみ。リトライ (Iceberg の再試行ループ) は
//!    呼び出し側の責務。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

/// 1 segment に入る block の最大数 (Databend の「最多 1000 个 block」)。
pub const MAX_BLOCKS_PER_SEGMENT: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TableFormatError {
    #[error("object {0} not found in the object store")]
    ObjectNotFound(String),
    #[error("failed to decode metadata at {path}: {message}")]
    Decode { path: String, message: String },
    #[error("commit conflict: table snapshot key is {actual:?}, expected {expected:?}")]
    CommitConflict { expected: Option<String>, actual: Option<String> },
    #[error("segment would hold {blocks} blocks, exceeding the limit of {limit}")]
    SegmentTooLarge { blocks: usize, limit: usize },
    #[error("snapshot {0} not found")]
    SnapshotNotFound(String),
}

/// オブジェクトストレージ (S3 互換) の最小インターフェース。
pub trait ObjectStore: Send + Sync {
    fn put(&self, path: &str, bytes: Vec<u8>);
    fn get(&self, path: &str) -> Option<Vec<u8>>;
    fn list(&self, prefix: &str) -> Vec<String>;
}

/// テスト・単一プロセス検証用のメモリ実装。
#[derive(Debug, Default)]
pub struct InMemoryObjectStore {
    objects: RwLock<BTreeMap<String, Vec<u8>>>,
}

impl InMemoryObjectStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.objects.read().expect("lock").len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl ObjectStore for InMemoryObjectStore {
    fn put(&self, path: &str, bytes: Vec<u8>) {
        self.objects.write().expect("lock").insert(path.to_string(), bytes);
    }
    fn get(&self, path: &str) -> Option<Vec<u8>> {
        self.objects.read().expect("lock").get(path).cloned()
    }
    fn list(&self, prefix: &str) -> Vec<String> {
        self.objects
            .read()
            .expect("lock")
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect()
    }
}

/// 数値列の min/max index。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColumnStats {
    pub min: f64,
    pub max: f64,
    pub null_count: u64,
}

/// 比較演算子 (枝刈り判定用)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeOp {
    Gt,
    Ge,
    Lt,
    Le,
}

impl ColumnStats {
    /// この統計から「述語を満たす行がこのブロックに絶対に存在しない」と
    /// 証明できるか。証明できない場合は `false` (= 読み飛ばさない) に倒す。
    pub fn disproves(&self, op: RangeOp, value: f64) -> bool {
        match op {
            RangeOp::Gt => self.max <= value,
            RangeOp::Ge => self.max < value,
            RangeOp::Lt => self.min >= value,
            RangeOp::Le => self.min > value,
        }
    }
}

/// 簡易 bloom filter (等値述語用)。偽陽性は許すが偽陰性は起こさない。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BloomFilter {
    bits: Vec<u8>,
}

impl BloomFilter {
    /// `bytes` バイトのビット配列で作る (既定32バイト = 256ビット)。
    pub fn with_size(bytes: usize) -> Self {
        Self { bits: vec![0u8; bytes.max(1)] }
    }

    pub fn new() -> Self {
        Self::with_size(32)
    }

    fn positions(&self, key: &[u8]) -> [usize; 2] {
        let digest = Sha256::digest(key);
        let total_bits = self.bits.len() * 8;
        let h1 = u64::from_le_bytes(digest[0..8].try_into().expect("8 bytes"));
        let h2 = u64::from_le_bytes(digest[8..16].try_into().expect("8 bytes"));
        [(h1 as usize) % total_bits, (h2 as usize) % total_bits]
    }

    pub fn insert(&mut self, key: &[u8]) {
        for p in self.positions(key) {
            self.bits[p / 8] |= 1 << (p % 8);
        }
    }

    /// `false` なら**確実に**含まれない。`true` は「含まれるかもしれない」。
    pub fn may_contain(&self, key: &[u8]) -> bool {
        self.positions(key).iter().all(|p| self.bits[p / 8] & (1 << (p % 8)) != 0)
    }
}

impl Default for BloomFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// block (実体は Parquet ファイル1つ) のメタデータ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockMeta {
    pub location: String,
    pub row_count: u64,
    pub size_bytes: u64,
    /// 数値列の min/max index
    pub column_stats: BTreeMap<String, ColumnStats>,
    /// 等値述語用の bloom filter (列名 -> フィルタ)
    pub bloom: BTreeMap<String, BloomFilter>,
}

impl BlockMeta {
    pub fn new(location: impl Into<String>, row_count: u64, size_bytes: u64) -> Self {
        Self {
            location: location.into(),
            row_count,
            size_bytes,
            column_stats: BTreeMap::new(),
            bloom: BTreeMap::new(),
        }
    }

    pub fn with_stats(mut self, column: &str, min: f64, max: f64, null_count: u64) -> Self {
        self.column_stats.insert(column.to_string(), ColumnStats { min, max, null_count });
        self
    }

    /// 列 `column` に値 `keys` が入っていることを bloom filter へ記録する。
    pub fn with_bloom<'a>(mut self, column: &str, keys: impl IntoIterator<Item = &'a str>) -> Self {
        let f = self.bloom.entry(column.to_string()).or_default();
        for k in keys {
            f.insert(k.as_bytes());
        }
        self
    }
}

/// segment (block の束) のメタデータ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentMeta {
    pub blocks: Vec<BlockMeta>,
    pub row_count: u64,
    /// segment 全体に集約した min/max (Databend が segment 側にも
    /// min/max を持つのと同じ——segment 単位で丸ごと読み飛ばせる)
    pub column_stats: BTreeMap<String, ColumnStats>,
}

impl SegmentMeta {
    pub fn from_blocks(blocks: Vec<BlockMeta>) -> Result<Self, TableFormatError> {
        if blocks.len() > MAX_BLOCKS_PER_SEGMENT {
            return Err(TableFormatError::SegmentTooLarge {
                blocks: blocks.len(),
                limit: MAX_BLOCKS_PER_SEGMENT,
            });
        }
        let row_count = blocks.iter().map(|b| b.row_count).sum();
        let mut column_stats: BTreeMap<String, ColumnStats> = BTreeMap::new();
        for b in &blocks {
            for (col, s) in &b.column_stats {
                column_stats
                    .entry(col.clone())
                    .and_modify(|agg| {
                        agg.min = agg.min.min(s.min);
                        agg.max = agg.max.max(s.max);
                        agg.null_count += s.null_count;
                    })
                    .or_insert(*s);
            }
        }
        Ok(Self { blocks, row_count, column_stats })
    }
}

/// snapshot (テーブルの1バージョン)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableSnapshot {
    pub snapshot_id: String,
    /// 直前のスナップショット (時間旅行のための連鎖)
    pub prev_snapshot_id: Option<String>,
    pub timestamp: i64,
    /// segment ファイルのパス一覧
    pub segments: Vec<String>,
    pub row_count: u64,
}

/// Databend の MetaSrv 相当。「テーブル -> 現在のスナップショットID」の
/// マップを持ち、**CAS が成功して初めてコミット成立**とする。
#[derive(Debug, Default)]
pub struct MetaService {
    keys: RwLock<BTreeMap<String, String>>,
}

impl MetaService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current(&self, table_key: &str) -> Option<String> {
        self.keys.read().expect("lock").get(table_key).cloned()
    }

    /// 楽観的 CAS。`expected` が現在値と一致しなければ `CommitConflict`。
    pub fn compare_and_swap(
        &self,
        table_key: &str,
        expected: Option<&str>,
        new: &str,
    ) -> Result<(), TableFormatError> {
        let mut keys = self.keys.write().expect("lock");
        let actual = keys.get(table_key).cloned();
        if actual.as_deref() != expected {
            return Err(TableFormatError::CommitConflict {
                expected: expected.map(|s| s.to_string()),
                actual,
            });
        }
        keys.insert(table_key.to_string(), new.to_string());
        Ok(())
    }
}

/// オブジェクトストレージ上の1テーブル。
pub struct ObjectTable {
    store: Arc<dyn ObjectStore>,
    meta: Arc<MetaService>,
    db_id: u64,
    table_id: u64,
    root: String,
}

impl std::fmt::Debug for ObjectTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectTable")
            .field("db_id", &self.db_id)
            .field("table_id", &self.table_id)
            .field("root", &self.root)
            .finish()
    }
}

impl ObjectTable {
    pub fn new(
        store: Arc<dyn ObjectStore>,
        meta: Arc<MetaService>,
        root: impl Into<String>,
        db_id: u64,
        table_id: u64,
    ) -> Self {
        Self { store, meta, db_id, table_id, root: root.into() }
    }

    /// MetaSrv 上のこのテーブルのキー。
    pub fn table_key(&self) -> String {
        format!("{}/{}/{}", self.root, self.db_id, self.table_id)
    }

    fn id_of(bytes: &[u8]) -> String {
        hex::encode(&Sha256::digest(bytes)[..16]) // 32桁16進 (Databend と同じ桁数)
    }

    fn segment_path(&self, id: &str) -> String {
        format!("{}/{}/{}/_sg/{id}_v1.json", self.root, self.db_id, self.table_id)
    }

    fn snapshot_path(&self, id: &str) -> String {
        format!("{}/{}/{}/_ss/{id}_v1.json", self.root, self.db_id, self.table_id)
    }

    /// block 群を1 segment として書き、新しいスナップショットを作って
    /// **MetaSrv の CAS が成功したら**コミット成立とする。
    /// 返り値は新しいスナップショットID。
    pub fn commit_blocks(&self, blocks: Vec<BlockMeta>) -> Result<String, TableFormatError> {
        let base = self.meta.current(&self.table_key());
        self.commit_blocks_onto(blocks, base.as_deref())
    }

    /// 明示した親スナップショットの上へコミットする (衝突検証用)。
    pub fn commit_blocks_onto(
        &self,
        blocks: Vec<BlockMeta>,
        expected_base: Option<&str>,
    ) -> Result<String, TableFormatError> {
        let segment = SegmentMeta::from_blocks(blocks)?;
        let seg_json = serde_json::to_vec(&segment)
            .map_err(|e| TableFormatError::Decode { path: "<segment>".into(), message: e.to_string() })?;
        let seg_id = Self::id_of(&seg_json);
        let seg_path = self.segment_path(&seg_id);
        self.store.put(&seg_path, seg_json);

        // 親スナップショットの segment 一覧を引き継ぐ (追記型)
        let (mut segments, mut row_count) = match expected_base {
            Some(id) => {
                let prev = self.load_snapshot(id)?;
                (prev.segments, prev.row_count)
            }
            None => (Vec::new(), 0),
        };
        segments.push(seg_path);
        row_count += segment.row_count;

        let timestamp = chrono::Utc::now().timestamp_millis();
        let mut snapshot = TableSnapshot {
            snapshot_id: String::new(),
            prev_snapshot_id: expected_base.map(|s| s.to_string()),
            timestamp,
            segments,
            row_count,
        };
        // ID は内容 + 親 + 時刻から決める (コンテンツアドレッサブル)
        let body = serde_json::to_vec(&snapshot)
            .map_err(|e| TableFormatError::Decode { path: "<snapshot>".into(), message: e.to_string() })?;
        snapshot.snapshot_id = Self::id_of(&body);
        let snap_json = serde_json::to_vec(&snapshot)
            .map_err(|e| TableFormatError::Decode { path: "<snapshot>".into(), message: e.to_string() })?;
        self.store.put(&self.snapshot_path(&snapshot.snapshot_id), snap_json);

        // ここが成否の分かれ目: MetaSrv の Snapshot Key の CAS。
        // 失敗した場合、書いたオブジェクトは孤児 (orphan) として残る
        // ——Databend も同様に GC (vacuum) の対象として扱う。
        self.meta
            .compare_and_swap(&self.table_key(), expected_base, &snapshot.snapshot_id)?;
        Ok(snapshot.snapshot_id)
    }

    pub fn load_snapshot(&self, id: &str) -> Result<TableSnapshot, TableFormatError> {
        let path = self.snapshot_path(id);
        let bytes = self
            .store
            .get(&path)
            .ok_or_else(|| TableFormatError::SnapshotNotFound(id.to_string()))?;
        serde_json::from_slice(&bytes)
            .map_err(|e| TableFormatError::Decode { path, message: e.to_string() })
    }

    pub fn load_segment(&self, path: &str) -> Result<SegmentMeta, TableFormatError> {
        let bytes = self
            .store
            .get(path)
            .ok_or_else(|| TableFormatError::ObjectNotFound(path.to_string()))?;
        serde_json::from_slice(&bytes)
            .map_err(|e| TableFormatError::Decode { path: path.to_string(), message: e.to_string() })
    }

    /// 現在コミット済みのスナップショット。
    pub fn current_snapshot(&self) -> Result<Option<TableSnapshot>, TableFormatError> {
        match self.meta.current(&self.table_key()) {
            Some(id) => Ok(Some(self.load_snapshot(&id)?)),
            None => Ok(None),
        }
    }

    /// スナップショットの連鎖を新しい順にたどる (時間旅行 / Time Travel)。
    pub fn snapshot_history(&self) -> Result<Vec<TableSnapshot>, TableFormatError> {
        let mut out = Vec::new();
        let mut cur = self.meta.current(&self.table_key());
        while let Some(id) = cur {
            let snap = self.load_snapshot(&id)?;
            cur = snap.prev_snapshot_id.clone();
            out.push(snap);
        }
        Ok(out)
    }

    /// 指定スナップショット時点の全 block を列挙する。
    pub fn blocks_at(&self, snapshot_id: &str) -> Result<Vec<BlockMeta>, TableFormatError> {
        let snap = self.load_snapshot(snapshot_id)?;
        let mut out = Vec::new();
        for seg_path in &snap.segments {
            out.extend(self.load_segment(seg_path)?.blocks);
        }
        Ok(out)
    }

    /// 範囲述語 `column op value` で、**読む必要のある block だけ**を返す。
    /// segment 単位の統計で丸ごと読み飛ばせる場合は segment ファイル内の
    /// block を一切見ない (Databend が segment 側にも min/max を持つ理由)。
    /// 戻り値は `(残った block, 読み飛ばした segment 数, 読み飛ばした block 数)`。
    pub fn prune_range(
        &self,
        snapshot_id: &str,
        column: &str,
        op: RangeOp,
        value: f64,
    ) -> Result<(Vec<BlockMeta>, usize, usize), TableFormatError> {
        let snap = self.load_snapshot(snapshot_id)?;
        let mut kept = Vec::new();
        let mut skipped_segments = 0usize;
        let mut skipped_blocks = 0usize;
        for seg_path in &snap.segments {
            let seg = self.load_segment(seg_path)?;
            if seg.column_stats.get(column).is_some_and(|s| s.disproves(op, value)) {
                skipped_segments += 1;
                skipped_blocks += seg.blocks.len();
                continue;
            }
            for b in seg.blocks {
                if b.column_stats.get(column).is_some_and(|s| s.disproves(op, value)) {
                    skipped_blocks += 1;
                    continue;
                }
                kept.push(b);
            }
        }
        Ok((kept, skipped_segments, skipped_blocks))
    }

    /// 等値述語 `column = key` を bloom filter で枝刈りする。
    /// bloom filter を持たない block は安全側に倒して残す。
    pub fn prune_equality(
        &self,
        snapshot_id: &str,
        column: &str,
        key: &str,
    ) -> Result<Vec<BlockMeta>, TableFormatError> {
        let mut kept = Vec::new();
        for b in self.blocks_at(snapshot_id)? {
            let keep = match b.bloom.get(column) {
                Some(f) => f.may_contain(key.as_bytes()),
                None => true,
            };
            if keep {
                kept.push(b);
            }
        }
        Ok(kept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> (Arc<InMemoryObjectStore>, Arc<MetaService>, ObjectTable) {
        let store = Arc::new(InMemoryObjectStore::new());
        let meta = Arc::new(MetaService::new());
        let t = ObjectTable::new(store.clone(), meta.clone(), "aruaru", 1, 42);
        (store, meta, t)
    }

    #[test]
    fn commit_creates_snapshot_segment_and_meta_key() {
        let (store, meta, t) = table();
        assert!(t.current_snapshot().unwrap().is_none());
        let id = t
            .commit_blocks(vec![BlockMeta::new("b1.parquet", 10, 100).with_stats("v", 1.0, 9.0, 0)])
            .unwrap();
        assert_eq!(meta.current(&t.table_key()).as_deref(), Some(id.as_str()));
        let snap = t.current_snapshot().unwrap().unwrap();
        assert_eq!(snap.row_count, 10);
        assert_eq!(snap.segments.len(), 1);
        assert!(snap.segments[0].contains("/_sg/"), "segment path: {}", snap.segments[0]);
        // snapshot と segment の2オブジェクトが書かれている
        assert_eq!(store.list("aruaru/1/42/").len(), 2);
        assert_eq!(t.blocks_at(&id).unwrap().len(), 1);
    }

    #[test]
    fn snapshots_chain_for_time_travel() {
        let (_s, _m, t) = table();
        let v1 = t.commit_blocks(vec![BlockMeta::new("b1", 10, 100)]).unwrap();
        let v2 = t.commit_blocks(vec![BlockMeta::new("b2", 5, 50)]).unwrap();
        assert_eq!(t.load_snapshot(&v2).unwrap().prev_snapshot_id.as_deref(), Some(v1.as_str()));
        // v1 時点は10行1ブロック、v2 時点は15行2ブロック (追記型)
        assert_eq!(t.load_snapshot(&v1).unwrap().row_count, 10);
        assert_eq!(t.load_snapshot(&v2).unwrap().row_count, 15);
        assert_eq!(t.blocks_at(&v1).unwrap().len(), 1);
        assert_eq!(t.blocks_at(&v2).unwrap().len(), 2);
        let hist = t.snapshot_history().unwrap();
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].snapshot_id, v2);
        assert_eq!(hist[1].snapshot_id, v1);
    }

    #[test]
    fn concurrent_commit_on_a_stale_base_is_rejected() {
        let (_s, meta, t) = table();
        let v1 = t.commit_blocks(vec![BlockMeta::new("b1", 1, 1)]).unwrap();
        // 別の書き手が先にコミット
        let v2 = t.commit_blocks(vec![BlockMeta::new("b2", 1, 1)]).unwrap();
        // v1 を親と思い込んでいる書き手は CAS で失敗する
        let err = t.commit_blocks_onto(vec![BlockMeta::new("b3", 1, 1)], Some(&v1)).unwrap_err();
        assert_eq!(
            err,
            TableFormatError::CommitConflict {
                expected: Some(v1.clone()),
                actual: Some(v2.clone())
            }
        );
        // MetaSrv の現在値は v2 のまま (壊れていない)
        assert_eq!(meta.current(&t.table_key()).as_deref(), Some(v2.as_str()));
    }

    #[test]
    fn first_commit_requires_no_existing_key() {
        let (_s, _m, t) = table();
        t.commit_blocks_onto(vec![BlockMeta::new("b1", 1, 1)], None).unwrap();
        // すでにキーがある状態で expected=None は衝突
        let err = t.commit_blocks_onto(vec![BlockMeta::new("b2", 1, 1)], None).unwrap_err();
        assert!(matches!(err, TableFormatError::CommitConflict { expected: None, .. }));
    }

    #[test]
    fn segment_stats_aggregate_block_stats() {
        let seg = SegmentMeta::from_blocks(vec![
            BlockMeta::new("b1", 2, 1).with_stats("v", 1.0, 5.0, 1),
            BlockMeta::new("b2", 3, 1).with_stats("v", 10.0, 20.0, 2),
        ])
        .unwrap();
        assert_eq!(seg.row_count, 5);
        let s = seg.column_stats.get("v").copied().unwrap();
        assert_eq!((s.min, s.max, s.null_count), (1.0, 20.0, 3));
    }

    #[test]
    fn segment_rejects_more_than_1000_blocks() {
        let blocks: Vec<BlockMeta> =
            (0..=MAX_BLOCKS_PER_SEGMENT).map(|i| BlockMeta::new(format!("b{i}"), 1, 1)).collect();
        let err = SegmentMeta::from_blocks(blocks).unwrap_err();
        assert_eq!(
            err,
            TableFormatError::SegmentTooLarge { blocks: 1001, limit: MAX_BLOCKS_PER_SEGMENT }
        );
    }

    #[test]
    fn range_pruning_skips_whole_segments_then_blocks() {
        let (_s, _m, t) = table();
        // segment1: v 1..9 / segment2: v 100..200
        t.commit_blocks(vec![
            BlockMeta::new("s1b1", 1, 1).with_stats("v", 1.0, 5.0, 0),
            BlockMeta::new("s1b2", 1, 1).with_stats("v", 6.0, 9.0, 0),
        ])
        .unwrap();
        let v2 = t
            .commit_blocks(vec![
                BlockMeta::new("s2b1", 1, 1).with_stats("v", 100.0, 150.0, 0),
                BlockMeta::new("s2b2", 1, 1).with_stats("v", 151.0, 200.0, 0),
            ])
            .unwrap();

        // v > 50: segment1 が丸ごと読み飛ばされる (block を見ない)
        let (kept, skipped_seg, skipped_blk) = t.prune_range(&v2, "v", RangeOp::Gt, 50.0).unwrap();
        assert_eq!(kept.len(), 2);
        assert_eq!((skipped_seg, skipped_blk), (1, 2));

        // v > 155: segment1 は丸ごと、segment2 は片方の block だけ残る
        let (kept, skipped_seg, skipped_blk) = t.prune_range(&v2, "v", RangeOp::Gt, 155.0).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].location, "s2b2");
        assert_eq!((skipped_seg, skipped_blk), (1, 3));

        // 統計を持たない列では枝刈りしない (安全側)
        let (kept, s1, s2) = t.prune_range(&v2, "unknown", RangeOp::Gt, 0.0).unwrap();
        assert_eq!((kept.len(), s1, s2), (4, 0, 0));
    }

    #[test]
    fn bloom_filter_has_no_false_negatives() {
        let mut f = BloomFilter::with_size(64);
        for k in ["alice", "bob", "carol"] {
            f.insert(k.as_bytes());
        }
        for k in ["alice", "bob", "carol"] {
            assert!(f.may_contain(k.as_bytes()), "{k} must never be reported absent");
        }
        // 偽陽性はあり得るので「必ず false」とは主張しない。
        // 多数の未挿入キーのうち大半が除外できることだけを確認する。
        let excluded = (0..200)
            .filter(|i| !f.may_contain(format!("absent-{i}").as_bytes()))
            .count();
        assert!(excluded > 150, "expected most absent keys to be excluded, got {excluded}/200");
    }

    #[test]
    fn equality_pruning_uses_bloom_filters() {
        let (_s, _m, t) = table();
        let v1 = t
            .commit_blocks(vec![
                BlockMeta::new("b_users_a", 2, 1).with_bloom("name", ["alice", "bob"]),
                BlockMeta::new("b_users_b", 2, 1).with_bloom("name", ["carol", "dave"]),
                BlockMeta::new("b_no_index", 2, 1),
            ])
            .unwrap();
        let kept = t.prune_equality(&v1, "name", "alice").unwrap();
        // alice を持つ block と、索引が無く判断できない block は残る
        assert!(kept.iter().any(|b| b.location == "b_users_a"));
        assert!(kept.iter().any(|b| b.location == "b_no_index"));
        assert!(kept.len() <= 3);
    }

    #[test]
    fn missing_snapshot_is_reported() {
        let (_s, _m, t) = table();
        assert_eq!(
            t.load_snapshot("deadbeef").unwrap_err(),
            TableFormatError::SnapshotNotFound("deadbeef".to_string())
        );
    }
}
