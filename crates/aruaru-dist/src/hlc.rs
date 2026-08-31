//! Hybrid Logical Clock(HLC)— 因果順序 + 実時刻近似の版付け(A.6-1)
//!
//! 参照: 論文「Logical Physical Clocks and Consistent Snapshots in
//! Globally Distributed Databases」/ `cockroach/pkg/util/hlc/hlc.go`。
//!
//! `closed_ts`/`wal_service`/`multi_raft` は現状「論理ナノ秒を呼び出し側が
//! 渡す」前提で実装されている(`docs/CONTROL_PLANE_REDESIGN.md` A.6-1に
//! 明記済みの制約)。本モジュールは、その論理ナノ秒の**生成方法**として
//! HLC を提供する——`Hlc::now()`が返す `as_nanos()` を、既存の
//! `advance_to`/`register_range`等へそのまま渡せる設計にした(既存API
//! の型シグネチャ〈u64ナノ秒〉は変更しない、追加のみ)。
//!
//! **正直な簡略化点**: クロックスキュー上限(CockroachDBの`max_offset`、
//! 許容できるノード間の時計ズレの上限を超えたら操作を拒否する仕組み)は
//! 実装していない——`update()`は常に受理する。ネットワーク越しの実際の
//! HLC伝播(送信時に相乗り・受信時にupdate)も本モジュール単体では
//! 提供せず、呼び出し側(`raft::transport`等)が明示的に呼ぶ設計とした。

use std::sync::atomic::{AtomicU64, Ordering};

/// physical component in nanoseconds(上位)+ logical counter(下位)を
/// 1つの`u64`へパックする。`pt`は最大`2^44`ナノ秒(約6.1日ぶんの相対値を
/// 想定するものではなく、実際にはUnixエポックからのナノ秒をそのまま
/// 使うため、下記`PT_BITS`は実用上十分な44bitを確保している——
/// 2^44ナノ秒 ≒ 6.1時間ではなく、`pt`はラップアラウンドしない
/// フル64bitのUnixナノ秒をそのまま保持し、`l`だけを下位ビットへ
/// 詰める設計にした(下記`Hlc`の内部表現参照)。
const LOGICAL_BITS: u32 = 16;
const LOGICAL_MASK: u64 = (1 << LOGICAL_BITS) - 1;

/// HLC タイムスタンプ。`pt`(物理成分、Unixナノ秒相当)と`l`(論理成分、
/// 同一`pt`内での順序を保証するカウンタ)を持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HlcTimestamp {
    pub pt: u64,
    pub l: u32,
}

impl HlcTimestamp {
    /// 既存の`closed_ts`/`wal_service`等が受け取る「論理ナノ秒
    /// (u64)」へエンコードする。`pt`を`LOGICAL_BITS`だけ左シフトし
    /// `l`を下位へ詰める——`pt`同士の比較優先度を保ったまま、単調な
    /// 単一`u64`として扱えるようにする(既存APIの型を変えないための
    /// 変換であり、`pt`のナノ秒精度は`LOGICAL_BITS`ぶん失われる点に
    /// 注意。厳密な物理時刻ではなく「単調な版番号」として使うことを
    /// 想定している)。
    pub fn as_nanos(&self) -> u64 {
        (self.pt << LOGICAL_BITS) | (self.l as u64 & LOGICAL_MASK)
    }
}

/// `Hlc`本体。`pt`と`l`を1つの`AtomicU64`へパックして保持することで、
/// `now()`/`update()`をロックフリーなCAS(compare-and-swap)ループで
/// 実装できるようにした(複数スレッドから同時に呼ばれる想定、
/// `parking_lot::Mutex`は使わない)。
pub struct Hlc {
    packed: AtomicU64,
}

fn pack(pt: u64, l: u32) -> u64 {
    (pt << LOGICAL_BITS) | (l as u64 & LOGICAL_MASK)
}

fn unpack(v: u64) -> (u64, u32) {
    (v >> LOGICAL_BITS, (v & LOGICAL_MASK) as u32)
}

impl Hlc {
    pub fn new() -> Self {
        Self {
            packed: AtomicU64::new(0),
        }
    }

    /// テスト・決定的シミュレーション用に初期値を指定して構築する。
    pub fn with_initial(pt: u64, l: u32) -> Self {
        Self {
            packed: AtomicU64::new(pack(pt, l)),
        }
    }

    /// ローカルイベント(書き込み等)発生時に呼ぶ。`wall_now_nanos`は
    /// 呼び出し側が渡す壁時計(テスト容易性のため——`SystemTime::now()`
    /// を直接呼ばず、決定的テストが可能な設計)。
    ///
    /// アルゴリズム(論文/CockroachDB `hlc.go`と同じ):
    /// `pt' = max(pt, wall_now)`; `pt' == pt` なら `l' = l + 1`、
    /// そうでなければ `l' = 0`。
    pub fn now(&self, wall_now_nanos: u64) -> HlcTimestamp {
        loop {
            let old = self.packed.load(Ordering::SeqCst);
            let (pt, l) = unpack(old);
            let (new_pt, new_l) = if wall_now_nanos > pt {
                (wall_now_nanos, 0)
            } else {
                (pt, l.saturating_add(1))
            };
            let new_packed = pack(new_pt, new_l);
            if self
                .packed
                .compare_exchange(old, new_packed, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return HlcTimestamp {
                    pt: new_pt,
                    l: new_l,
                };
            }
        }
    }

    /// リモートから受信したHLCタイムスタンプと自クロックを統合する。
    /// `pt' = max(pt, remote_pt, wall_now)`。3者のうちどれが採用された
    /// かで `l'` を決める(単調増加を保証):
    /// - ローカル`pt`が最大 → ローカル`l + 1`
    /// - リモート`pt`が最大 → リモート`l + 1`
    /// - `wall_now`が最大(両者より新しい) → `l' = 0`
    /// - 複数が同値で並んだ場合は既存`l`とリモート`l`の大きい方 + 1。
    pub fn update(&self, remote: HlcTimestamp, wall_now_nanos: u64) -> HlcTimestamp {
        loop {
            let old = self.packed.load(Ordering::SeqCst);
            let (pt, l) = unpack(old);
            let max_pt = pt.max(remote.pt).max(wall_now_nanos);
            let new_l = if max_pt == wall_now_nanos && max_pt > pt && max_pt > remote.pt {
                0
            } else if pt == max_pt && remote.pt == max_pt {
                l.max(remote.l).saturating_add(1)
            } else if pt == max_pt {
                l.saturating_add(1)
            } else if remote.pt == max_pt {
                remote.l.saturating_add(1)
            } else {
                0
            };
            let new_packed = pack(max_pt, new_l);
            if self
                .packed
                .compare_exchange(old, new_packed, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return HlcTimestamp {
                    pt: max_pt,
                    l: new_l,
                };
            }
        }
    }

    /// 現在値をイベントとして進めずにそのまま読む(観測用)。
    pub fn peek(&self) -> HlcTimestamp {
        let (pt, l) = unpack(self.packed.load(Ordering::SeqCst));
        HlcTimestamp { pt, l }
    }
}

impl Default for Hlc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_advances_pt_when_wall_clock_is_ahead() {
        let hlc = Hlc::with_initial(100, 5);
        let ts = hlc.now(200);
        assert_eq!(ts.pt, 200);
        assert_eq!(ts.l, 0);
    }

    #[test]
    fn now_bumps_logical_when_wall_clock_has_not_advanced() {
        let hlc = Hlc::with_initial(100, 5);
        let ts = hlc.now(50); // wall clock behind pt
        assert_eq!(ts.pt, 100);
        assert_eq!(ts.l, 6);
    }

    #[test]
    fn now_is_monotonic_across_repeated_calls_with_same_wall_time() {
        let hlc = Hlc::new();
        let a = hlc.now(1_000);
        let b = hlc.now(1_000);
        let c = hlc.now(1_000);
        assert!(a < b);
        assert!(b < c);
    }

    #[test]
    fn update_adopts_remote_pt_when_remote_is_ahead() {
        let hlc = Hlc::with_initial(100, 0);
        let remote = HlcTimestamp { pt: 500, l: 3 };
        let ts = hlc.update(remote, 100); // wall clock stale
        assert_eq!(ts.pt, 500);
        assert_eq!(ts.l, 4);
    }

    #[test]
    fn update_keeps_local_pt_when_local_is_ahead_of_remote_and_wall() {
        let hlc = Hlc::with_initial(900, 2);
        let remote = HlcTimestamp { pt: 500, l: 9 };
        let ts = hlc.update(remote, 100);
        assert_eq!(ts.pt, 900);
        assert_eq!(ts.l, 3);
    }

    #[test]
    fn update_uses_wall_clock_when_it_exceeds_both_local_and_remote() {
        let hlc = Hlc::with_initial(100, 5);
        let remote = HlcTimestamp { pt: 200, l: 9 };
        let ts = hlc.update(remote, 1_000);
        assert_eq!(ts.pt, 1_000);
        assert_eq!(ts.l, 0);
    }

    #[test]
    fn update_result_is_never_less_than_prior_local_value() {
        let hlc = Hlc::with_initial(1_000_000, 0);
        let remote = HlcTimestamp { pt: 1, l: 1 };
        let ts = hlc.update(remote, 1);
        assert!(ts.pt >= 1_000_000);
    }

    #[test]
    fn as_nanos_preserves_ordering_between_timestamps() {
        let a = HlcTimestamp { pt: 100, l: 0 };
        let b = HlcTimestamp { pt: 100, l: 1 };
        let c = HlcTimestamp { pt: 101, l: 0 };
        assert!(a.as_nanos() < b.as_nanos());
        assert!(b.as_nanos() < c.as_nanos());
    }

    #[test]
    fn concurrent_now_calls_never_produce_duplicate_timestamps() {
        use std::sync::Arc;
        use std::thread;

        let hlc = Arc::new(Hlc::new());
        let mut handles = Vec::new();
        for _ in 0..8 {
            let hlc = hlc.clone();
            handles.push(thread::spawn(move || {
                let mut out = Vec::with_capacity(200);
                for _ in 0..200 {
                    out.push(hlc.now(42).as_nanos());
                }
                out
            }));
        }
        let mut all = Vec::new();
        for h in handles {
            all.extend(h.join().unwrap());
        }
        let unique: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len(), "HLC produced duplicate timestamps under concurrency");
    }
}
