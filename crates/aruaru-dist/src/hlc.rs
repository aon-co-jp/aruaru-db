//! Hybrid Logical Clock(HLC)— 因果順序 + 実時刻近似の版付け(A.6-1)
//!
//! 参照: 論文「Logical Physical Clocks and Consistent Snapshots in
//! Globally Distributed Databases」/ `cockroach/pkg/util/hlc/hlc.go`。
//!
//! **2026-09-02 再設計(正本: `docs/HLC_TIMESTAMP_REDESIGN.md`)**:
//! 旧実装の `as_nanos()` は `self.pt << 16` を行っており、`pt` に実 Unix
//! ナノ秒(≈ 2⁶⁰·⁶)を入れると u64 オーバーフローする設計ミスがあった。
//! 一次資料調査(CockroachDB は物理/論理を別フィールドで持ちパックしない、
//! compact な 1整数版は物理成分を ms/µs 粒度へ落として下位ビットを空ける)を
//! 踏まえ、**物理成分を 65.536µs 粒度(下位 16bit をゼロ)へ切り捨てて
//! その 16bit に論理カウンタを収める**方式へ改めた。左シフトも桁上げも
//! 発生しないためオーバーフローせず、厳密な単調増加を保つ。
//!
//! `closed_ts`/`wal_service`/`multi_raft` は「論理ナノ秒(u64)を呼び出し側が
//! 渡す」前提で実装されている。本モジュールは、その u64 の**生成方法**として
//! HLC を提供する——`Hlc::now(wall).as_ordinal()` を、既存の `advance_to`/
//! `register_range` 等へそのまま渡せる(既存 API の型シグネチャは不変)。
//!
//! **正直な簡略化点**:
//! - 物理分解能は 65.536µs(旧コメントが暗に主張していたナノ秒精度は
//!   失われる)。HLC は「厳密な物理時刻」ではなく「因果順序を保った単調な
//!   版番号 + 実時刻の近似」であり、`closed_ts` の `target_lag` は秒単位
//!   (既定 3 秒)のため実用上の影響は無い。
//! - クロックスキュー上限(CockroachDB の `max_offset`)は未実装
//!   ——`update()` はリモート値を常に受理する。
//! - ネットワーク越しの HLC 伝播(送信時に相乗り・受信時に update)は
//!   呼び出し側(`raft::transport` / `admin.rs::closed_ts_receive` 等)が
//!   明示的に `update` / `observe_ordinal` を呼ぶ設計。

use std::sync::atomic::{AtomicU64, Ordering};

/// 論理カウンタに割り当てる下位ビット数。1 バケット = 2^16 ナノ秒 ≒ 65.536µs。
const LOGICAL_BITS: u32 = 16;
const LOGICAL_MASK: u64 = (1 << LOGICAL_BITS) - 1; // 0xFFFF
const PHYSICAL_MASK: u64 = !LOGICAL_MASK; // 0xFFFF_FFFF_FFFF_0000
/// 論理カウンタが枯渇した時に物理成分を進める1ステップ。
const BUCKET: u64 = LOGICAL_MASK + 1; // 0x1_0000

/// 壁時計ナノ秒を「バケット」へ切り捨てる(下位 16bit をゼロに)。
#[inline]
fn bucket(wall_nanos: u64) -> u64 {
    wall_nanos & PHYSICAL_MASK
}

/// HLC タイムスタンプ。`pt`(物理成分、**バケット整列済み Unix ナノ秒**
/// = 下位 16bit がゼロ)と `l`(論理成分、同一バケット内での順序カウンタ、
/// 0..=65535)を持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HlcTimestamp {
    pub pt: u64,
    pub l: u32,
}

impl HlcTimestamp {
    /// 既存の `closed_ts`/`wal_service` 等が受け取る「単調な u64 版番号」へ
    /// エンコードする。`pt` の下位 16bit(必ずゼロのはずだが外部構築の
    /// `HlcTimestamp` に対しても安全なようマスクする)へ `l` を詰めるだけ
    /// ——**左シフトしないためオーバーフローしない**。同一バケット内は `l`
    /// で順序が付き、バケットが進めば `pt` が 65536 以上ジャンプするため、
    /// `l`(< 65536)がどれだけ大きくても次バケットを超えられない
    /// (厳密単調)。
    pub fn as_ordinal(&self) -> u64 {
        (self.pt & PHYSICAL_MASK) | (self.l as u64 & LOGICAL_MASK)
    }

    /// 旧名。`as_ordinal()` へ委譲する後方互換エイリアス。
    #[deprecated(note = "renamed to as_ordinal(); the old impl shifted pt<<16 and overflowed for real Unix nanos")]
    pub fn as_nanos(&self) -> u64 {
        self.as_ordinal()
    }

    /// u64 ordinal を `HlcTimestamp` へ復号する(side transport 受信時など)。
    pub fn from_ordinal(ordinal: u64) -> Self {
        Self {
            pt: ordinal & PHYSICAL_MASK,
            l: (ordinal & LOGICAL_MASK) as u32,
        }
    }
}

/// `Hlc` 本体。`pt` と `l` を 1 つの `AtomicU64`(= ordinal そのもの)へ
/// 保持することで、`now()`/`update()` をロックフリーな CAS ループで実装
/// できる(`parking_lot::Mutex` は使わない)。
pub struct Hlc {
    /// `as_ordinal()` と同じ表現(上位 = バケット整列 pt、下位 16bit = l)。
    packed: AtomicU64,
    /// クロックスキュー上限(ナノ秒、CockroachDB の `max_offset` 相当)。
    /// `0` = 無効(リモート値を常に受理、従来挙動)。有効時、
    /// リモートの物理成分が壁時計より `max_offset` 以上先なら
    /// `try_update` は `Err(ClockSkew)` を返し、`update` はリモート値を
    /// 無視してローカル進行のみ行う。
    max_offset_nanos: AtomicU64,
}

/// `try_update` / `try_observe_ordinal` がスキュー上限を超えたリモート値を
/// 拒否したときの詳細。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockSkew {
    /// リモートの物理成分(バケット整列済み)。
    pub remote_pt: u64,
    /// 判定に使った壁時計(バケット整列済み)。
    pub wall_bucket: u64,
    /// 設定されているスキュー上限(ナノ秒)。
    pub max_offset_nanos: u64,
}

impl std::fmt::Display for ClockSkew {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "remote HLC pt {} is more than max_offset {}ns ahead of local wall bucket {}",
            self.remote_pt, self.max_offset_nanos, self.wall_bucket
        )
    }
}
impl std::error::Error for ClockSkew {}

#[inline]
fn unpack(v: u64) -> (u64, u32) {
    (v & PHYSICAL_MASK, (v & LOGICAL_MASK) as u32)
}

#[inline]
fn pack(pt: u64, l: u32) -> u64 {
    (pt & PHYSICAL_MASK) | (l as u64 & LOGICAL_MASK)
}

/// `now()` の遷移: バケットが進めば `(b, 0)`、同一バケットで論理に余りが
/// あれば `(pt, l+1)`、論理が枯渇したら次バケットを先食いして `(pt+BUCKET, 0)`。
fn advance_local(pt: u64, l: u32, wall_now_nanos: u64) -> (u64, u32) {
    let b = bucket(wall_now_nanos);
    if b > pt {
        (b, 0)
    } else if (l as u64) < LOGICAL_MASK {
        (pt, l + 1)
    } else {
        (pt + BUCKET, 0)
    }
}

impl Hlc {
    pub fn new() -> Self {
        Self {
            packed: AtomicU64::new(0),
            max_offset_nanos: AtomicU64::new(0),
        }
    }

    /// テスト・決定的シミュレーション用に初期値を指定して構築する。
    /// `pt` はバケット整列される(下位 16bit は捨てられる)。
    pub fn with_initial(pt: u64, l: u32) -> Self {
        Self {
            packed: AtomicU64::new(pack(pt, l)),
            max_offset_nanos: AtomicU64::new(0),
        }
    }

    /// クロックスキュー上限(ナノ秒)を設定して構築する。`0` = 無効。
    pub fn with_max_offset_nanos(n: u64) -> Self {
        let h = Self::new();
        h.set_max_offset_nanos(n);
        h
    }

    /// クロックスキュー上限(ナノ秒)を実行時に設定する。`0` = 無効。
    pub fn set_max_offset_nanos(&self, n: u64) {
        self.max_offset_nanos.store(n, Ordering::SeqCst);
    }

    /// 現在のクロックスキュー上限(ナノ秒、`0` = 無効)。
    pub fn max_offset_nanos(&self) -> u64 {
        self.max_offset_nanos.load(Ordering::SeqCst)
    }

    /// リモート物理成分が壁時計 + `max_offset` を超えていないか検査する。
    fn skew_ok(&self, remote_pt: u64, wall_now_nanos: u64) -> Result<(), ClockSkew> {
        let max_offset = self.max_offset_nanos.load(Ordering::SeqCst);
        if max_offset == 0 {
            return Ok(());
        }
        let wall_bucket = bucket(wall_now_nanos);
        let remote_b = bucket(remote_pt);
        if remote_b > wall_bucket.saturating_add(max_offset) {
            Err(ClockSkew {
                remote_pt: remote_b,
                wall_bucket,
                max_offset_nanos: max_offset,
            })
        } else {
            Ok(())
        }
    }

    /// ローカルイベント(書き込み等)発生時に呼ぶ。`wall_now_nanos` は
    /// 呼び出し側が渡す壁時計(テスト容易性のため——`SystemTime::now()` を
    /// 直接呼ばず、決定的テストが可能な設計。実運用は [`Hlc::now_ordinal`])。
    pub fn now(&self, wall_now_nanos: u64) -> HlcTimestamp {
        loop {
            let old = self.packed.load(Ordering::SeqCst);
            let (pt, l) = unpack(old);
            let (new_pt, new_l) = advance_local(pt, l, wall_now_nanos);
            let new_packed = pack(new_pt, new_l);
            // new_packed <= old のときは他スレッドが先に進めた——読み直す。
            if new_packed <= old {
                // 他スレッドが既に our 提案以上へ進めているので、単に再試行して
                // 最新値からもう一度進める(単調性は保たれる)。
                std::hint::spin_loop();
                continue;
            }
            if self
                .packed
                .compare_exchange(old, new_packed, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return HlcTimestamp { pt: new_pt, l: new_l };
            }
        }
    }

    /// `SystemTime::now()` を内部で読んで `now(wall).as_ordinal()` を返す
    /// 実運用向けの便利メソッド。
    pub fn now_ordinal(&self) -> u64 {
        let wall = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        self.now(wall).as_ordinal()
    }

    /// リモートから受信した HLC タイムスタンプと自クロックを統合する。
    /// `remote.pt` はバケット整列して扱う。物理成分は 3 者
    /// (ローカル pt / リモート pt / 壁時計バケット)の最大、論理成分は
    /// どれが採用されたかで決める(単調増加を保証)。論理が枯渇したら
    /// 物理を次バケットへ桁上げする。
    pub fn update(&self, remote: HlcTimestamp, wall_now_nanos: u64) -> HlcTimestamp {
        // スキュー上限を超えたリモート値はクロックを汚染しないよう無視し、
        // ローカル進行だけ行う(`try_update` は代わりに Err を返す)。
        if self.skew_ok(remote.pt, wall_now_nanos).is_err() {
            tracing::warn!(
                remote_pt = bucket(remote.pt),
                wall = bucket(wall_now_nanos),
                max_offset_ns = self.max_offset_nanos.load(Ordering::SeqCst),
                "HLC update: remote timestamp exceeds max_offset; ignoring remote, advancing locally only"
            );
            return self.now(wall_now_nanos);
        }
        let remote_b = bucket(remote.pt);
        loop {
            let old = self.packed.load(Ordering::SeqCst);
            let (pt, l) = unpack(old);
            let b = bucket(wall_now_nanos);
            let max_pt = pt.max(remote_b).max(b);
            let mut new_l = if max_pt == b && b > pt && b > remote_b {
                0
            } else if pt == max_pt && remote_b == max_pt {
                l.max(remote.l).saturating_add(1)
            } else if pt == max_pt {
                l.saturating_add(1)
            } else if remote_b == max_pt {
                remote.l.saturating_add(1)
            } else {
                0
            };
            let mut new_pt = max_pt;
            if (new_l as u64) > LOGICAL_MASK {
                new_pt += BUCKET;
                new_l = 0;
            }
            let new_packed = pack(new_pt, new_l);
            if new_packed <= old {
                // 既にローカルが提案以上——`old` をそのまま採用(単調性維持)。
                return HlcTimestamp { pt, l };
            }
            if self
                .packed
                .compare_exchange(old, new_packed, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return HlcTimestamp { pt: new_pt, l: new_l };
            }
        }
    }

    /// `update` のスキュー検査版。上限を超えたリモート値なら
    /// `Err(ClockSkew)` を返す(クロックは進めない)。
    pub fn try_update(
        &self,
        remote: HlcTimestamp,
        wall_now_nanos: u64,
    ) -> Result<HlcTimestamp, ClockSkew> {
        self.skew_ok(remote.pt, wall_now_nanos)?;
        Ok(self.update(remote, wall_now_nanos))
    }

    /// side transport 等から届いた u64 ordinal を復号して `update` する。
    /// 戻り値は統合後のローカル HLC。
    pub fn observe_ordinal(&self, remote_ordinal: u64, wall_now_nanos: u64) -> HlcTimestamp {
        self.update(HlcTimestamp::from_ordinal(remote_ordinal), wall_now_nanos)
    }

    /// `observe_ordinal` のスキュー検査版。
    pub fn try_observe_ordinal(
        &self,
        remote_ordinal: u64,
        wall_now_nanos: u64,
    ) -> Result<HlcTimestamp, ClockSkew> {
        self.try_update(HlcTimestamp::from_ordinal(remote_ordinal), wall_now_nanos)
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
    fn as_ordinal_never_overflows_with_real_unix_nanos() {
        // 2026 年相当の実 Unix ナノ秒。旧実装は pt<<16 でここが panic/ラップした。
        let real_nanos: u64 = 1_760_000_000_000_000_000;
        let ts = HlcTimestamp { pt: bucket(real_nanos), l: 5 };
        let ord = ts.as_ordinal(); // panic しないこと
        assert!(ord < u64::MAX);
        // ordinal は真の壁時計から ±65535ns 以内。
        assert!(ord >= real_nanos - LOGICAL_MASK && ord <= real_nanos + LOGICAL_MASK);
    }

    #[test]
    fn now_advances_bucket_when_wall_clock_moves_to_a_new_bucket() {
        let hlc = Hlc::with_initial(0x10_0000, 5);
        let ts = hlc.now(0x20_0000 + 1234); // 別バケット
        assert_eq!(ts.pt, 0x20_0000);
        assert_eq!(ts.l, 0);
    }

    #[test]
    fn now_bumps_logical_within_the_same_bucket() {
        let hlc = Hlc::with_initial(0x10_0000, 5);
        let ts = hlc.now(0x10_0000 + 42); // 同一バケット(下位16bitは切り捨てられる)
        assert_eq!(ts.pt, 0x10_0000);
        assert_eq!(ts.l, 6);
    }

    #[test]
    fn now_is_monotonic_across_repeated_calls_with_same_wall_time() {
        let hlc = Hlc::new();
        let a = hlc.now(1_000_000).as_ordinal();
        let b = hlc.now(1_000_000).as_ordinal();
        let c = hlc.now(1_000_000).as_ordinal();
        assert!(a < b && b < c, "{a} < {b} < {c}");
    }

    #[test]
    fn ordinal_is_monotonic_across_bucket_boundary_even_with_large_logical() {
        let hlc = Hlc::new();
        let mut prev = 0u64;
        // 同一バケットで論理を 60000 まで回す。
        for _ in 0..60_000 {
            let o = hlc.now(0x30_0000 + 7).as_ordinal();
            assert!(o > prev);
            prev = o;
        }
        // 壁時計を 1 バケット進める → ordinal は跳ねるが逆転はしない。
        let after = hlc.now(0x40_0000).as_ordinal();
        assert!(after > prev, "bucket boundary must not invert: {after} !> {prev}");
        assert_eq!(after & LOGICAL_MASK, 0, "new bucket resets logical to 0");
    }

    #[test]
    fn now_carries_into_next_bucket_when_logical_saturates() {
        let hlc = Hlc::with_initial(0x50_0000, 0);
        let mut prev = 0u64;
        // 同一壁時計で 70000 回 → 途中で論理枯渇 → 次バケットへ桁上げ。
        let mut carried = false;
        for _ in 0..70_000 {
            let ts = hlc.now(0x50_0000);
            let o = ts.as_ordinal();
            assert!(o > prev, "monotonic through saturation: {o} !> {prev}");
            prev = o;
            if ts.pt > 0x50_0000 {
                carried = true;
            }
        }
        assert!(carried, "logical counter should have carried into the next bucket");
    }

    #[test]
    fn update_adopts_remote_bucket_when_remote_is_ahead() {
        let hlc = Hlc::with_initial(0x10_0000, 0);
        let remote = HlcTimestamp { pt: 0x50_0000, l: 3 };
        let ts = hlc.update(remote, 0x10_0000); // 壁時計は据え置き
        assert_eq!(ts.pt, 0x50_0000);
        assert_eq!(ts.l, 4);
    }

    #[test]
    fn update_keeps_local_when_local_is_ahead_of_remote_and_wall() {
        let hlc = Hlc::with_initial(0x90_0000, 2);
        let remote = HlcTimestamp { pt: 0x50_0000, l: 9 };
        let ts = hlc.update(remote, 0x10_0000);
        assert_eq!(ts.pt, 0x90_0000);
        assert_eq!(ts.l, 3);
    }

    #[test]
    fn update_uses_wall_bucket_when_it_exceeds_both() {
        let hlc = Hlc::with_initial(0x10_0000, 5);
        let remote = HlcTimestamp { pt: 0x20_0000, l: 9 };
        let ts = hlc.update(remote, 0x99_0000 + 123);
        assert_eq!(ts.pt, 0x99_0000);
        assert_eq!(ts.l, 0);
    }

    #[test]
    fn update_result_is_never_less_than_prior_local_value() {
        let hlc = Hlc::with_initial(0x10_0000_0000, 0);
        let before = hlc.peek().as_ordinal();
        let remote = HlcTimestamp { pt: 0x1_0000, l: 1 };
        let after = hlc.update(remote, 0x1).as_ordinal();
        assert!(after >= before);
    }

    #[test]
    fn observe_ordinal_pulls_local_clock_forward() {
        let hlc = Hlc::with_initial(0x10_0000, 0);
        // ローカルより十分先の ordinal を観測。
        let remote_ord = (0x80_0000u64 & PHYSICAL_MASK) | 7;
        hlc.observe_ordinal(remote_ord, 0x10_0000);
        let next = hlc.now(0x10_0000).as_ordinal();
        assert!(next > remote_ord, "local clock must advance past an observed remote ordinal");
    }

    #[test]
    fn as_ordinal_preserves_ordering_between_bucket_aligned_timestamps() {
        let a = HlcTimestamp { pt: 0x10_0000, l: 0 };
        let b = HlcTimestamp { pt: 0x10_0000, l: 1 };
        let c = HlcTimestamp { pt: 0x20_0000, l: 0 };
        assert!(a.as_ordinal() < b.as_ordinal());
        assert!(b.as_ordinal() < c.as_ordinal());
    }

    #[test]
    fn max_offset_rejects_a_remote_timestamp_from_the_far_future() {
        // 上限 1ms。壁時計は小さいバケット、リモートは遥か先。
        let hlc = Hlc::with_max_offset_nanos(1_000_000);
        let wall = 0x10_0000u64;
        let far_future = HlcTimestamp { pt: 0x9000_0000, l: 3 };

        let err = hlc.try_update(far_future, wall).expect_err("should reject far-future remote");
        assert_eq!(err.max_offset_nanos, 1_000_000);

        // 汚染されていない: permissive update もローカル進行のみ。
        let local = hlc.update(far_future, wall);
        assert!(local.pt <= bucket(wall) + BUCKET, "clock not poisoned by rejected remote");

        // 上限内のリモートは通常どおり受理。
        let near = HlcTimestamp { pt: wall + 0x2_0000, l: 1 };
        assert!(hlc.try_update(near, wall).is_ok());
    }

    #[test]
    fn max_offset_zero_disables_the_check_backward_compatible() {
        let hlc = Hlc::new(); // 既定は上限無効
        assert_eq!(hlc.max_offset_nanos(), 0);
        let far = HlcTimestamp { pt: 0x9000_0000, l: 0 };
        assert!(hlc.try_update(far, 0x10_0000).is_ok(), "disabled check accepts anything");
        assert_eq!(hlc.peek().pt, 0x9000_0000);
    }

    #[test]
    fn concurrent_now_calls_never_produce_duplicate_ordinals() {
        use std::sync::Arc;
        use std::thread;

        let hlc = Arc::new(Hlc::new());
        let mut handles = Vec::new();
        for _ in 0..8 {
            let hlc = hlc.clone();
            handles.push(thread::spawn(move || {
                let mut out = Vec::with_capacity(500);
                for _ in 0..500 {
                    out.push(hlc.now(1_048_576).as_ordinal());
                }
                out
            }));
        }
        let mut all = Vec::new();
        for h in handles {
            all.extend(h.join().unwrap());
        }
        let unique: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len(), "HLC produced duplicate ordinals under concurrency");
    }
}
