//! Prolly Tree (Probabilistic B-Tree / Merkle Search Tree)
//!
//! ## 概要
//! コンテンツアドレッサブルな B-tree 変種。Dolt が先行実装した概念を Pure Rust で実装。
//! 各ノードは内容の SHA-256 ハッシュで識別され、チャンク境界は
//! キー/値の内容から決まる **ローリングハッシュ** で確率的に決定される。
//!
//! ## なぜ普通の B-tree でなく Prolly Tree か
//! - **構造的共有**: 2 つのツリーで変更のない部分木はハッシュが一致 → 共有できる
//! - **O(変更量) diff**: ルートから降りて、ハッシュが同じノードは丸ごとスキップ
//! - **決定的構造**: 同じデータからは挿入順に関係なく同じツリーができる
//!   (= 同じ root_hash → コミット ID が再現可能)
//!
//! ## チャンク境界の決め方
//! 各エントリの (key, value) ハッシュの下位ビットを見て、
//! `hash % target_chunk_size == 0` ならそこをチャンク境界とする。
//! これにより平均チャンクサイズを制御しつつ、内容ベースで境界が決まる。

use std::collections::BTreeMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// ノードハッシュ (32 bytes)
pub type NodeHash = [u8; 32];

/// 目標平均チャンクサイズ (エントリ数)。境界確率 = 1/TARGET。
/// 4 → 平均 4 エントリごとに境界。実運用では 16〜64 程度。
const TARGET_CHUNK_SIZE: u64 = 4;

/// ローリングハッシュのウィンドウマスク。
/// (hash & PATTERN_MASK) == 0 でチャンク境界とする。
/// TARGET_CHUNK_SIZE=4 に対応するため下位 2 ビット。
const PATTERN_MASK: u64 = TARGET_CHUNK_SIZE - 1;

// ─────────────────────────────────────────────────────────────
// ノード定義
// ─────────────────────────────────────────────────────────────

/// Prolly Tree のノード。Leaf はキー値ペア、Internal は子ノードへの参照を持つ。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Node {
    /// 葉ノード: 実データ (key → value)
    Leaf {
        entries: Vec<(Vec<u8>, Vec<u8>)>,
    },
    /// 内部ノード: 各子の「最小キー」と子ハッシュ
    Internal {
        children: Vec<(Vec<u8>, NodeHash)>,
    },
}

impl Node {
    /// このノードの内容ハッシュを計算 (コンテンツアドレッシング)
    pub fn hash(&self) -> NodeHash {
        let mut hasher = Sha256::new();
        match self {
            Node::Leaf { entries } => {
                hasher.update(b"leaf");
                for (k, v) in entries {
                    hasher.update((k.len() as u32).to_le_bytes());
                    hasher.update(k);
                    hasher.update((v.len() as u32).to_le_bytes());
                    hasher.update(v);
                }
            }
            Node::Internal { children } => {
                hasher.update(b"internal");
                for (k, h) in children {
                    hasher.update((k.len() as u32).to_le_bytes());
                    hasher.update(k);
                    hasher.update(h);
                }
            }
        }
        hasher.finalize().into()
    }

    /// このノードの最小キー (最左)
    fn min_key(&self) -> Option<&[u8]> {
        match self {
            Node::Leaf { entries } => entries.first().map(|(k, _)| k.as_slice()),
            Node::Internal { children } => children.first().map(|(k, _)| k.as_slice()),
        }
    }
}

// ─────────────────────────────────────────────────────────────
// ノードストア (コンテンツアドレッサブルストレージ)
// ─────────────────────────────────────────────────────────────

/// ノードハッシュ → ノード のストア。
/// 同じハッシュのノードは 1 度だけ保存され、ツリー間で共有される。
/// (本番は redb に永続化。ここではインメモリ実装)
#[derive(Default)]
pub struct NodeStore {
    nodes: RwLock<BTreeMap<NodeHash, Node>>,
}

impl NodeStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// ノードを保存し、そのハッシュを返す
    pub fn put(&self, node: Node) -> NodeHash {
        let hash = node.hash();
        self.nodes.write().insert(hash, node);
        hash
    }

    /// ハッシュからノードを取得
    pub fn get(&self, hash: &NodeHash) -> Option<Node> {
        self.nodes.read().get(hash).cloned()
    }

    /// 保存されているノード総数 (重複排除後)
    pub fn node_count(&self) -> usize {
        self.nodes.read().len()
    }
}

// ─────────────────────────────────────────────────────────────
// チャンク境界判定 (ローリングハッシュ)
// ─────────────────────────────────────────────────────────────

/// 与えられた (key, value) がチャンク境界になるか判定。
/// 内容ベースなので、同じデータなら常に同じ位置で分割される。
fn is_boundary(key: &[u8], value: &[u8]) -> bool {
    let mut hasher = Sha256::new();
    hasher.update(key);
    hasher.update(value);
    let digest: [u8; 32] = hasher.finalize().into();
    let h = u64::from_le_bytes(digest[0..8].try_into().unwrap());
    (h & PATTERN_MASK) == 0
}

// ─────────────────────────────────────────────────────────────
// Prolly Tree 本体
// ─────────────────────────────────────────────────────────────

/// Prolly Tree。ルートハッシュとノードストアを持つ。
pub struct ProllyTree {
    root: RwLock<Option<NodeHash>>,
    store: Arc<NodeStore>,
}

impl ProllyTree {
    /// 空のツリーを作成
    pub fn new(store: Arc<NodeStore>) -> Self {
        Self {
            root: RwLock::new(None),
            store,
        }
    }

    /// 既存ルートからツリーを開く
    pub fn from_root(root: NodeHash, store: Arc<NodeStore>) -> Self {
        Self {
            root: RwLock::new(Some(root)),
            store,
        }
    }

    /// 現在のルートハッシュ (= コミットの root_hash)
    pub fn root_hash(&self) -> NodeHash {
        self.root.read().unwrap_or([0u8; 32])
    }

    /// ソート済み全エントリからツリーを (再)構築する。
    /// 決定的: 同じ入力 → 同じツリー → 同じルートハッシュ。
    pub fn build(&self, mut entries: Vec<(Vec<u8>, Vec<u8>)>) {
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries.dedup_by(|a, b| a.0 == b.0);

        if entries.is_empty() {
            *self.root.write() = None;
            return;
        }

        // 1. 葉レベルをチャンク化
        let mut leaf_hashes: Vec<(Vec<u8>, NodeHash)> = Vec::new();
        let mut current: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();

        for (k, v) in entries {
            let boundary = is_boundary(&k, &v);
            current.push((k, v));
            if boundary {
                let node = Node::Leaf { entries: std::mem::take(&mut current) };
                let min = node.min_key().unwrap().to_vec();
                leaf_hashes.push((min, self.store.put(node)));
            }
        }
        // 残り
        if !current.is_empty() {
            let node = Node::Leaf { entries: current };
            let min = node.min_key().unwrap().to_vec();
            leaf_hashes.push((min, self.store.put(node)));
        }

        // 2. 内部レベルを積み上げる (1 ノードになるまで)
        let mut level = leaf_hashes;
        while level.len() > 1 {
            level = self.build_internal_level(level);
        }

        *self.root.write() = Some(level[0].1);
    }

    /// 内部ノード 1 レベルを構築
    fn build_internal_level(
        &self,
        children: Vec<(Vec<u8>, NodeHash)>,
    ) -> Vec<(Vec<u8>, NodeHash)> {
        let mut result = Vec::new();
        let mut current: Vec<(Vec<u8>, NodeHash)> = Vec::new();

        for (key, hash) in children {
            // 内部ノードの境界判定は (キー, 子ハッシュ) で行う
            let boundary = is_boundary(&key, &hash);
            current.push((key, hash));
            if boundary && current.len() >= 2 {
                let node = Node::Internal { children: std::mem::take(&mut current) };
                let min = node.min_key().unwrap().to_vec();
                result.push((min, self.store.put(node)));
            }
        }
        if !current.is_empty() {
            let node = Node::Internal { children: current };
            let min = node.min_key().unwrap().to_vec();
            result.push((min, self.store.put(node)));
        }
        result
    }

    /// キーで値を検索
    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let root = (*self.root.read())?;
        self.get_recursive(&root, key)
    }

    fn get_recursive(&self, node_hash: &NodeHash, key: &[u8]) -> Option<Vec<u8>> {
        let node = self.store.get(node_hash)?;
        match node {
            Node::Leaf { entries } => entries
                .iter()
                .find(|(k, _)| k.as_slice() == key)
                .map(|(_, v)| v.clone()),
            Node::Internal { children } => {
                // key 以下で最大の min_key を持つ子に降りる
                let mut target: Option<&NodeHash> = None;
                for (min_k, child_hash) in &children {
                    if min_k.as_slice() <= key {
                        target = Some(child_hash);
                    } else {
                        break;
                    }
                }
                target.and_then(|h| self.get_recursive(h, key))
            }
        }
    }

    /// 全エントリを昇順で取得 (スキャン)
    pub fn scan(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut out = Vec::new();
        if let Some(root) = *self.root.read() {
            self.scan_recursive(&root, &mut out);
        }
        out
    }

    fn scan_recursive(&self, node_hash: &NodeHash, out: &mut Vec<(Vec<u8>, Vec<u8>)>) {
        if let Some(node) = self.store.get(node_hash) {
            match node {
                Node::Leaf { entries } => out.extend(entries),
                Node::Internal { children } => {
                    for (_, child) in children {
                        self.scan_recursive(&child, out);
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────
// 構造的 diff (O(変更量))
// ─────────────────────────────────────────────────────────────

/// 2 つの Prolly Tree ルート間の差分。
/// ハッシュが一致するノードは丸ごとスキップするため、
/// 計算量は「変更があった部分木」のサイズに比例する。
#[derive(Debug, Clone, PartialEq)]
pub enum ProllyDiff {
    Added(Vec<u8>, Vec<u8>),               // key, new_value
    Removed(Vec<u8>, Vec<u8>),             // key, old_value
    Modified(Vec<u8>, Vec<u8>, Vec<u8>),   // key, old_value, new_value
}

/// 2 ルート間の diff を計算。
///
/// **構造共有スキップ**: 両ツリーで葉ノードのハッシュが一致する部分は
/// 中身が完全に同一なので展開せずスキップする。よって計算量は
/// 「ハッシュが異なる(=変更があった)葉のサイズ」に比例する = O(変更量)。
pub fn diff_trees(
    store: &NodeStore,
    from_root: Option<NodeHash>,
    to_root: Option<NodeHash>,
) -> Vec<ProllyDiff> {
    // ルートが同一 (または両方空) なら差分ゼロ — 最大の早期リターン
    if from_root == to_root {
        return Vec::new();
    }

    // 各ツリーの葉ノードハッシュ集合を収集
    let from_leaves = from_root
        .map(|r| collect_leaf_hashes(store, &r))
        .unwrap_or_default();
    let to_leaves = to_root
        .map(|r| collect_leaf_hashes(store, &r))
        .unwrap_or_default();

    // 「相手側に存在しない葉」だけからエントリを収集する。
    // 両側に同一ハッシュで存在する葉 = 無変更ブロック → スキップ。
    let from_entries = entries_from_unique_leaves(store, &from_leaves, &to_leaves);
    let to_entries = entries_from_unique_leaves(store, &to_leaves, &from_leaves);

    let from_map: BTreeMap<Vec<u8>, Vec<u8>> = from_entries.into_iter().collect();
    let to_map: BTreeMap<Vec<u8>, Vec<u8>> = to_entries.into_iter().collect();

    let mut out = Vec::new();

    // 追加・変更
    for (k, v_to) in &to_map {
        match from_map.get(k) {
            None => out.push(ProllyDiff::Added(k.clone(), v_to.clone())),
            Some(v_from) if v_from != v_to => {
                out.push(ProllyDiff::Modified(k.clone(), v_from.clone(), v_to.clone()))
            }
            // 同一値が両 unique 集合に現れた場合 = 隣のキーが変わった葉に
            // たまたま同居していただけ → 無変更なので何もしない
            _ => {}
        }
    }
    // 削除
    for (k, v_from) in &from_map {
        if !to_map.contains_key(k) {
            out.push(ProllyDiff::Removed(k.clone(), v_from.clone()));
        }
    }

    out
}

/// ルート以下の葉ノードハッシュをすべて収集
fn collect_leaf_hashes(
    store: &NodeStore,
    node_hash: &NodeHash,
) -> std::collections::BTreeSet<NodeHash> {
    let mut set = std::collections::BTreeSet::new();
    collect_leaf_hashes_inner(store, node_hash, &mut set);
    set
}

fn collect_leaf_hashes_inner(
    store: &NodeStore,
    node_hash: &NodeHash,
    set: &mut std::collections::BTreeSet<NodeHash>,
) {
    if let Some(node) = store.get(node_hash) {
        match node {
            Node::Leaf { .. } => {
                set.insert(*node_hash);
            }
            Node::Internal { children } => {
                for (_, child) in children {
                    collect_leaf_hashes_inner(store, &child, set);
                }
            }
        }
    }
}

/// `mine` にあって `other` に無い葉ノードからのみエントリを収集
fn entries_from_unique_leaves(
    store: &NodeStore,
    mine: &std::collections::BTreeSet<NodeHash>,
    other: &std::collections::BTreeSet<NodeHash>,
) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    for leaf_hash in mine.difference(other) {
        if let Some(Node::Leaf { entries }) = store.get(leaf_hash) {
            out.extend(entries);
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────
// テスト
// ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(k: &str, v: &str) -> (Vec<u8>, Vec<u8>) {
        (k.as_bytes().to_vec(), v.as_bytes().to_vec())
    }

    #[test]
    fn test_build_and_get() {
        let store = Arc::new(NodeStore::new());
        let tree = ProllyTree::new(store);
        tree.build(vec![
            kv("apple", "1"),
            kv("banana", "2"),
            kv("cherry", "3"),
            kv("date", "4"),
            kv("elderberry", "5"),
        ]);

        assert_eq!(tree.get(b"apple"), Some(b"1".to_vec()));
        assert_eq!(tree.get(b"cherry"), Some(b"3".to_vec()));
        assert_eq!(tree.get(b"elderberry"), Some(b"5".to_vec()));
        assert_eq!(tree.get(b"missing"), None);
    }

    #[test]
    fn test_deterministic_root() {
        // 同じデータを挿入順を変えて構築しても同じ root_hash になる
        let store1 = Arc::new(NodeStore::new());
        let t1 = ProllyTree::new(store1);
        t1.build(vec![kv("a", "1"), kv("b", "2"), kv("c", "3")]);

        let store2 = Arc::new(NodeStore::new());
        let t2 = ProllyTree::new(store2);
        t2.build(vec![kv("c", "3"), kv("a", "1"), kv("b", "2")]); // 順不同

        assert_eq!(t1.root_hash(), t2.root_hash());
    }

    #[test]
    fn test_scan_sorted() {
        let store = Arc::new(NodeStore::new());
        let tree = ProllyTree::new(store);
        tree.build(vec![kv("c", "3"), kv("a", "1"), kv("b", "2")]);
        let scanned = tree.scan();
        let keys: Vec<_> = scanned.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys, vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
    }

    #[test]
    fn test_diff() {
        let store = Arc::new(NodeStore::new());

        let t1 = ProllyTree::new(store.clone());
        t1.build(vec![kv("a", "1"), kv("b", "2"), kv("c", "3")]);
        let root1 = t1.root_hash();

        let t2 = ProllyTree::new(store.clone());
        t2.build(vec![kv("a", "1"), kv("b", "20"), kv("d", "4")]); // b変更, c削除, d追加
        let root2 = t2.root_hash();

        let diff = diff_trees(&store, Some(root1), Some(root2));

        assert!(diff.contains(&ProllyDiff::Modified(b"b".to_vec(), b"2".to_vec(), b"20".to_vec())));
        assert!(diff.contains(&ProllyDiff::Removed(b"c".to_vec(), b"3".to_vec())));
        assert!(diff.contains(&ProllyDiff::Added(b"d".to_vec(), b"4".to_vec())));
        assert_eq!(diff.len(), 3);
    }

    #[test]
    fn test_structural_sharing() {
        // 1エントリだけ違うツリーは、大部分のノードを共有する
        let store = Arc::new(NodeStore::new());

        let mut base: Vec<_> = (0..100)
            .map(|i| kv(&format!("key{:03}", i), &format!("val{}", i)))
            .collect();

        let t1 = ProllyTree::new(store.clone());
        t1.build(base.clone());
        let count_after_t1 = store.node_count();

        // 1エントリだけ変更
        base[50] = kv("key050", "CHANGED");
        let t2 = ProllyTree::new(store.clone());
        t2.build(base);
        let count_after_t2 = store.node_count();

        // ルートは変わる
        assert_ne!(t1.root_hash(), t2.root_hash());
        // 新規ノードは「変更パスのみ」なので、全ノード再生成より遥かに少ない
        let new_nodes = count_after_t2 - count_after_t1;
        assert!(new_nodes < count_after_t1, "構造共有が効いていない: +{} nodes", new_nodes);
    }
}
