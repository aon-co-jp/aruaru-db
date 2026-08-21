//! ScyllaDB/Seastar型 shard-per-core(shared-nothing)ストア
//!
//! 【見送り再検証(2026-08-21)の経緯】前回HANDOFFはScyllaDBのshard-per-core
//! アーキテクチャを「tokioランタイム全体をSeastar型イベントループへ置き換える
//! 規模の書き換えが必要」として実装コストのみを理由に見送っていた。
//! ユーザー指示により再検証したところ、この判断は「全体を置き換える」という
//! 過大なスコープを前提にしており、**設計思想の核だけを切り出して部分適用する**
//! という現実的な選択肢を検討していなかったことが判明した。
//!
//! 参考にした一次情報源(2026-08-21再調査):
//! - https://www.scylladb.com/product/technology/shard-per-core-architecture/
//! - https://seastar.io/shared-nothing/
//! - https://www.scylladb.com/2024/10/21/why-scylladbs-shard-per-core-architecture-matters/
//!
//! ## 移植した核心的な設計思想
//!
//! ScyllaDB/Seastarの核心は「(1) データをコアごとに完全分割し、(2) コア間の
//! 通信を共有メモリ+ロックではなく明示的なメッセージパッシングに限定する」
//! という2点(`shared-nothing`)。本モジュールはこの2点だけを、`tokio`
//! ランタイム自体は一切置き換えずに実現する:
//!
//! - `ShardedRowStore<V>`は内部に`shard_count`個の独立したシャードを持つ。
//!   各シャードは専用の`std::thread`(Seastarの「1コアに1スレッド」に対応、
//!   ただし本実装はOSスレッドで、SeastarのようなCPUピニング・独自
//!   スケジューラまでは実装しない——下記「正直な開示」参照)上で、
//!   `HashMap<Vec<u8>, V>`を**そのスレッド以外からは一切直接アクセスしない**
//!   形で保持する。
//! - シャード間・呼び出し元とシャード間の通信は`std::sync::mpsc`
//!   (Rust標準ライブラリのMPSCチャネル)経由のメッセージパッシングのみ。
//!   `RwLock`/`Mutex`でデータそのものを共有する既存の`QueryEngine::tables`
//!   (`parking_lot::RwLock<HashMap<...>>`)とは対照的に、**このモジュールの
//!   データ本体には呼び出し元スレッドから直接アクセスできる手段が
//!   構造的に存在しない**(所有権自体がシャードスレッドの外に出ない)。
//! - キーからシャードへの割り当ては、ScyllaDBのtoken-aware routing
//!   (パーティションキーのハッシュ→token→vnode→物理シャード)の簡略版として、
//!   `SHA-256(key) % shard_count`の決定的ハッシュで行う(`aruaru-core`の
//!   既存ZFS互換チェックサム〈`compute_checksum`〉と同じSHA-256を再利用し、
//!   新規ハッシュアルゴリズムの追加を避けた)。
//!
//! ## 正直な開示・スコープの限界(誇張しない)
//!
//! 1. **CPUコアへのピニング(affinity)は行っていない**。Seastarは各スレッドを
//!    特定の物理コアへ明示的に固定し、OSスケジューラによる移動を防ぐことで
//!    キャッシュ局所性を最大化するが、本実装はOSのデフォルトスケジューラに
//!    任せる(Windows/Linux双方で移植性のあるCPUアフィニティAPIを本実装の
//!    スコープに含めなかった——将来`core_affinity`crate等の追加を検討する
//!    余地はある)。
//! 2. **専用I/Oキュー・独自スケジューラは無い**。SeastarはCPU/ディスク/
//!    ネットワークI/Oすべてをシャードごとに独立したイベントループで処理する
//!    フルスケールのランタイムだが、本実装はデータ分割とメッセージパッシング
//!    のみを移植し、I/Oスケジューリング自体は既存のtokio/OSに委ねる。
//! 3. **既存の`QueryEngine`の主経路は置き換えていない**。`QueryEngine::tables`
//!    (`parking_lot::RwLock`)は今回変更しておらず、`ShardedRowStore`は独立した
//!    新規コンポーネントとして追加した——本番の書き込み経路への配線
//!    (`QueryEngine`をシャード分割ストレージへ全面移行する)は、Raft・
//!    Prolly Tree・OLAPキャッシュ等の既存機構すべてがテーブル単位の単一
//!    HashMapを前提に設計されているため、影響範囲が非常に広く今回のスコープ
//!    には含めていない(次回以降の課題として`CLAUDE.md`に記録)。
//! 4. 上記の制約はあるが、「データをコアごとに独立分割し、コア間はロックでは
//!    なく明示的なメッセージパッシングのみで通信する」というShared-Nothingの
//!    **核心的な性質そのもの**は、以下のテストで直接検証している
//!    (ロック競合が構造的に存在しないこと自体をアーキテクチャで保証する)。

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread::JoinHandle;

use sha2::{Digest, Sha256};

/// シャードへ送るメッセージ(明示的なメッセージパッシングのみで通信する
/// ——共有可変状態への直接アクセス手段を持たない)。
enum ShardMsg<V> {
    Put(Vec<u8>, V, mpsc::Sender<()>),
    Get(Vec<u8>, mpsc::Sender<Option<V>>),
    Delete(Vec<u8>, mpsc::Sender<Option<V>>),
    Len(mpsc::Sender<usize>),
    Shutdown,
}

/// ScyllaDB shard-per-core方式の shared-nothing キー値ストア。
pub struct ShardedRowStore<V: Send + Clone + 'static> {
    shard_count: usize,
    senders: Vec<mpsc::Sender<ShardMsg<V>>>,
    handles: Vec<JoinHandle<()>>,
}

impl<V: Send + Clone + 'static> ShardedRowStore<V> {
    /// `shard_count`個の独立したシャードスレッドを起動する。
    /// ScyllaDBの既定(利用可能コア数と同数のシャード)に倣い、
    /// `shard_count`に0を渡した場合は`std::thread::available_parallelism`
    /// (このマシンの論理コア数)を使う。
    pub fn new(shard_count: usize) -> Self {
        let shard_count = if shard_count == 0 {
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
        } else {
            shard_count
        };

        let mut senders = Vec::with_capacity(shard_count);
        let mut handles = Vec::with_capacity(shard_count);

        for shard_id in 0..shard_count {
            let (tx, rx) = mpsc::channel::<ShardMsg<V>>();
            let handle = std::thread::Builder::new()
                .name(format!("aruaru-shard-{shard_id}"))
                .spawn(move || shard_loop(rx))
                .expect("failed to spawn shard thread");
            senders.push(tx);
            handles.push(handle);
        }

        Self { shard_count, senders, handles }
    }

    pub fn shard_count(&self) -> usize {
        self.shard_count
    }

    /// ScyllaDBのtoken-aware routingの簡略版: SHA-256(key) % shard_count で
    /// 決定的にシャードを選ぶ(同じキーは常に同じシャードへ)。
    pub fn shard_for(&self, key: &[u8]) -> usize {
        let mut hasher = Sha256::new();
        hasher.update(key);
        let digest = hasher.finalize();
        // 先頭8バイトをu64として使う(SHA-256の分布特性上、下位ビットだけでも
        // 十分に一様——ScyllaDBのtoken空間分割と同じ考え方)。
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&digest[..8]);
        (u64::from_be_bytes(buf) as usize) % self.shard_count
    }

    pub fn put(&self, key: Vec<u8>, value: V) {
        let shard = self.shard_for(&key);
        let (ack_tx, ack_rx) = mpsc::channel();
        self.senders[shard].send(ShardMsg::Put(key, value, ack_tx)).expect("shard thread gone");
        let _ = ack_rx.recv();
    }

    pub fn get(&self, key: &[u8]) -> Option<V> {
        let shard = self.shard_for(key);
        let (tx, rx) = mpsc::channel();
        self.senders[shard].send(ShardMsg::Get(key.to_vec(), tx)).expect("shard thread gone");
        rx.recv().ok().flatten()
    }

    pub fn delete(&self, key: &[u8]) -> Option<V> {
        let shard = self.shard_for(key);
        let (tx, rx) = mpsc::channel();
        self.senders[shard].send(ShardMsg::Delete(key.to_vec(), tx)).expect("shard thread gone");
        rx.recv().ok().flatten()
    }

    /// 全シャードの合計エントリ数(各シャードへ`Len`メッセージを送りgather)。
    pub fn total_len(&self) -> usize {
        let mut total = 0;
        for tx in &self.senders {
            let (reply_tx, reply_rx) = mpsc::channel();
            if tx.send(ShardMsg::Len(reply_tx)).is_ok() {
                total += reply_rx.recv().unwrap_or(0);
            }
        }
        total
    }

    /// シャード単体のエントリ数(どのシャードにどれだけデータが偏っているかの
    /// 観測用——ScyllaDBの"hot shard"検知に相当する用途)。
    pub fn shard_len(&self, shard_id: usize) -> usize {
        let (tx, rx) = mpsc::channel();
        self.senders[shard_id].send(ShardMsg::Len(tx)).expect("shard thread gone");
        rx.recv().unwrap_or(0)
    }
}

impl<V: Send + Clone + 'static> Drop for ShardedRowStore<V> {
    fn drop(&mut self) {
        for tx in &self.senders {
            let _ = tx.send(ShardMsg::Shutdown);
        }
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}

/// 1シャード分のイベントループ(専用スレッド上で実行、そのスレッド以外から
/// 直接アクセスされない`HashMap`を排他的に所有する)。
fn shard_loop<V: Clone>(rx: mpsc::Receiver<ShardMsg<V>>) {
    let mut map: HashMap<Vec<u8>, V> = HashMap::new();
    while let Ok(msg) = rx.recv() {
        match msg {
            ShardMsg::Put(k, v, ack) => {
                map.insert(k, v);
                let _ = ack.send(());
            }
            ShardMsg::Get(k, reply) => {
                let _ = reply.send(map.get(&k).cloned());
            }
            ShardMsg::Delete(k, reply) => {
                let _ = reply.send(map.remove(&k));
            }
            ShardMsg::Len(reply) => {
                let _ = reply.send(map.len());
            }
            ShardMsg::Shutdown => break,
        }
    }
}

// Get/Delete は値をチャネル越しに返すため Clone が要る(所有権をシャード
// スレッドの外へ持ち出す唯一の経路がメッセージのコピーであることの表れ)。
impl<V: Send + Clone + 'static> ShardMsg<V> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct Row(String);

    #[test]
    fn put_and_get_round_trip_across_shards() {
        let store: ShardedRowStore<Row> = ShardedRowStore::new(4);
        for i in 0..50 {
            store.put(format!("key{i}").into_bytes(), Row(format!("value{i}")));
        }
        for i in 0..50 {
            let got = store.get(format!("key{i}").as_bytes());
            assert_eq!(got, Some(Row(format!("value{i}"))));
        }
        assert_eq!(store.total_len(), 50);
    }

    /// 同じキーは常に同じシャードへ決定的にルーティングされる
    /// (ScyllaDBのtoken-aware routingが同じパーティションキーを常に
    /// 同じシャードへ送るのと同じ性質)。
    #[test]
    fn same_key_always_routes_to_the_same_shard() {
        let store: ShardedRowStore<Row> = ShardedRowStore::new(8);
        let shard_first = store.shard_for(b"stable-key");
        for _ in 0..20 {
            assert_eq!(store.shard_for(b"stable-key"), shard_first);
        }
    }

    /// データが実際に複数シャードへ分散されること(全件が1シャードへ
    /// 偏っていないこと)を確認する——分割自体が機能している直接証拠。
    #[test]
    fn keys_are_actually_distributed_across_multiple_shards() {
        let store: ShardedRowStore<Row> = ShardedRowStore::new(4);
        for i in 0..200 {
            store.put(format!("k{i}").into_bytes(), Row(i.to_string()));
        }
        let mut used_shards = std::collections::HashSet::new();
        for shard_id in 0..store.shard_count() {
            if store.shard_len(shard_id) > 0 {
                used_shards.insert(shard_id);
            }
        }
        assert!(used_shards.len() > 1, "200件のキーが1シャードだけに集中するのは分散として不自然(実際は{:?}シャードのみ使用)", used_shards.len());
        assert_eq!(store.total_len(), 200);
    }

    /// 削除の往復確認: deleteは削除した値を返し、以後get/total_lenへ反映される。
    #[test]
    fn delete_removes_the_entry_and_returns_the_old_value() {
        let store: ShardedRowStore<Row> = ShardedRowStore::new(3);
        store.put(b"a".to_vec(), Row("alpha".into()));
        assert_eq!(store.total_len(), 1);
        let removed = store.delete(b"a");
        assert_eq!(removed, Some(Row("alpha".into())));
        assert_eq!(store.get(b"a"), None);
        assert_eq!(store.total_len(), 0);
    }

    /// 【shared-nothingの核心特性】複数の呼び出し元スレッドから同時並行に
    /// 書き込んでも、各シャードは専用スレッドが排他的に処理するため
    /// (呼び出し元は`Mutex`等でシャード内部のHashMapを直接ロックしていない)、
    /// 最終的に全件が欠落なく反映される——メッセージパッシングだけで
    /// 安全な並行アクセスが成立していることの直接証拠。
    #[test]
    fn concurrent_writers_from_multiple_threads_all_land_without_data_races() {
        use std::sync::Arc;
        let store: Arc<ShardedRowStore<Row>> = Arc::new(ShardedRowStore::new(4));
        let mut handles = Vec::new();
        for t in 0..8 {
            let store = store.clone();
            handles.push(std::thread::spawn(move || {
                for i in 0..25 {
                    let key = format!("t{t}-k{i}");
                    store.put(key.clone().into_bytes(), Row(key));
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(store.total_len(), 8 * 25);
    }

    /// 0を渡すと利用可能な論理コア数を自動的に採用する(ScyllaDBの既定
    /// 「コア数と同数のシャード」を模した挙動)。
    #[test]
    fn zero_shard_count_falls_back_to_available_parallelism() {
        let store: ShardedRowStore<Row> = ShardedRowStore::new(0);
        assert!(store.shard_count() >= 1);
    }
}
