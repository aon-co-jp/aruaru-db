//! Hybrid Logical Clock(HLC)— 因果順序 + 実時刻近似の版付け(A.6-1)
//!
//! 参照: 論文「Logical Physical Clocks and Consistent Snapshots in
//! Globally Distributed Databases」/ `cockroach/pkg/util/hlc/hlc.go` /
//! `atolab/uhlc-rs`(Zenoh)。
//!
//! **2026-09-03 再設計 P-HLC-3(正本: `docs/HLC_TIMESTAMP_REDESIGN.md` §6)
//! = 案A への全面移行 + P-HLC-3d = 射影からシフト・パック・切り捨てを完全撤去**:
//!
//! 旧・案B(2026-09-02)は「物理成分を 65.536µs 粒度へ切り捨て、下位 16bit に
//! 論理カウンタを詰めて 1 個の `AtomicU64` に収める」方式だった。u64
//! オーバーフローは除去できたが、**物理分解能がナノ秒 → 65µs に落ちる**という
//! 代償があった。P-HLC-3(2026-09-03 前半)で内部表現はフル精度 2 フィールドへ
//! 刷新したが、**外向き `as_ordinal()` に案B の `& !0xFFFF` バケット射影が
//! 残っており**、`advance_locked` も「バケットが進んだか」で分岐していた
//! (= シフト・パックの残滓)。P-HLC-3d でこれを撤去した。
//!
//! 一次資料(CockroachDB `util/hlc` は `WallTime int64`・`Logical int32`・
//! `Synthetic bool` をパックせず別フィールド + `sync.Mutex`。uhlc-rs も
//! `Mutex`。両者とも incoming が壁時計 + max_offset を超えたら `Err`)に忠実:
//!
//! - 内部: [`HlcTimestamp`] = `{ wall_nanos: u64(フル精度 Unix ナノ秒、
//!   一切切り捨てない), logical: u32, synthetic: bool }`。順序は
//!   `(wall_nanos, logical)` の辞書式。
//! - クロック本体 [`Hlc`] は `parking_lot::Mutex` 保護(業界主流)。
//! - **`advance_locked` はバケット概念を持たない**: 物理クロックが 1ns でも
//!   進めば `wall_nanos` を採用し `logical = 0`、進んでいなければ `logical += 1`
//!   ——CockroachDB `hlc.go` `Now()` とビット単位で同じ規則。
//! - **`as_ordinal()` は `wall_nanos.saturating_add(logical)`**。シフトも
//!   マスクも無い。`logical` は「物理クロックが停まっている間に発生した
//!   同一ナノ秒内の追加イベント数」であり、ナノ秒空間へそのまま足し込む
//!   (= その分だけ先の synthetic な時刻。`max_offset` で有界)。切り捨てが
//!   無いので `closed_ts` 等が受け取る u64 は**フル精度の Unix ナノ秒**。
//! - u64 経路の**厳密単調**は [`Hlc`] が `last_ordinal` を記憶してクランプ
//!   (`logical` 畳み込みで逆転しかけたら `last_ordinal + 1`)。
//!   [`Hlc::now_ordinal`] / [`Hlc::observe_ordinal`] / [`Hlc::try_observe_ordinal`]
//!   のシグネチャは不変 ——`closed_ts` / `wal_service` / GraphQL
//!   `closedTsAdvance` は**一切変更不要**。
//! - [`HlcTimestamp::from_ordinal`] は `{ wall_nanos: ordinal, logical: 0,
//!   synthetic: true }`(物理成分をそのまま復元。畳み込まれた `logical` の
//!   端数は u64 経路では失うが、フル精度が要る所は [`Hlc::observe_hlc`])。
//! - 新 API: [`Hlc::now_hlc`] / [`Hlc::observe_hlc`] がフル精度
//!   [`HlcTimestamp`] を返す。[`HlcTimestamp::uncertainty_upper`] は
//!   CockroachDB の uncertainty interval 上端 `wall_nanos + max_offset`。
//!
//! ネットワーク越しの HLC 伝播(送信時に相乗り・受信時に update)は
//! 呼び出し側(`raft::transport` / `admin.rs::closed_ts_receive` 等)が
//! 明示的に `observe_ordinal` / `observe_hlc` を呼ぶ設計。

use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

/// HLC タイムスタンプ(案A、フル精度 2 フィールド)。
///
/// - `wall_nanos`: 物理成分。**切り捨てていない Unix エポックからのナノ秒**。
/// - `logical`: 論理成分。同一 `wall_nanos` 内での順序カウンタ、および
///   物理時刻が進んでいない間の因果前進に使う。
/// - `synthetic`: `wall_nanos` が実際の物理クロック読み値より先行しているか
///   (CockroachDB の `Synthetic` 準拠)。順序・等価比較では**無視**する。
#[derive(Debug, Clone, Copy)]
pub struct HlcTimestamp {
    pub wall_nanos: u64,
    pub logical: u32,
    pub synthetic: bool,
}

impl HlcTimestamp {
    /// `synthetic = false` の素の構築。
    pub fn new(wall_nanos: u64, logical: u32) -> Self {
        Self { wall_nanos, logical, synthetic: false }
    }

    /// 因果順序のキー。`synthetic` は含めない(CockroachDB `EqOrdering` と同じ)。
    #[inline]
    fn key(&self) -> (u64, u32) {
        (self.wall_nanos, self.logical)
    }

    /// 既存の `closed_ts` / `wal_service` 等が受け渡す「単調な u64 版番号」への
    /// **射影**(P-HLC-3d、シフト・マスク・バケット無し)。
    /// `wall_nanos.saturating_add(logical)` ——`wall_nanos` は切り捨てず、
    /// `logical`(= 物理クロック停止中に発生した同一ナノ秒内の追加イベント数)を
    /// ナノ秒空間へそのまま足し込むだけ。結果はフル精度の Unix ナノ秒スケール。
    ///
    /// 注意: 物理クロックが 1ns 進んで `logical` が 0 に戻った直後など、
    /// `as_ordinal()` 単体では前回値より小さくなり得る。u64 の**厳密単調**が
    /// 必要なら [`Hlc::now_ordinal`] を使うこと(`Hlc` が `last_ordinal` で
    /// クランプする)。
    pub fn as_ordinal(&self) -> u64 {
        self.wall_nanos.saturating_add(self.logical as u64)
    }

    /// 旧名。`as_ordinal()` へ委譲する後方互換エイリアス。
    #[deprecated(note = "renamed to as_ordinal(); the pre-2026-09-02 impl shifted pt<<16 and overflowed for real Unix nanos")]
    pub fn as_nanos(&self) -> u64 {
        self.as_ordinal()
    }

    /// u64 ordinal を `HlcTimestamp` へ復号(side transport 受信時など)。
    /// 物理成分をそのまま復元する(`synthetic = true`。u64 経路では畳み込まれた
    /// `logical` の端数は失われる ——フル精度が要る所は [`Hlc::observe_hlc`])。
    pub fn from_ordinal(ordinal: u64) -> Self {
        Self { wall_nanos: ordinal, logical: 0, synthetic: true }
    }

    /// CockroachDB の uncertainty interval 上端。`wall_nanos + max_offset`
    /// ——このタイムスタンプで読むトランザクションが「見落としているかも
    /// しれない書き込み」の時刻上限。follower read の staleness 判定に使える。
    pub fn uncertainty_upper(&self, max_offset_nanos: u64) -> u64 {
        self.wall_nanos.saturating_add(max_offset_nanos)
    }
}

impl PartialEq for HlcTimestamp {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}
impl Eq for HlcTimestamp {}
impl PartialOrd for HlcTimestamp {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for HlcTimestamp {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key().cmp(&other.key())
    }
}

/// `try_update` / `try_observe_ordinal` がスキュー上限を超えたリモート値を
/// 拒否したときの詳細。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockSkew {
    /// リモートの物理成分(フル精度ナノ秒)。
    pub remote_wall_nanos: u64,
    /// 判定に使ったローカル壁時計(フル精度ナノ秒)。
    pub local_wall_nanos: u64,
    /// 設定されているスキュー上限(ナノ秒)。
    pub max_offset_nanos: u64,
}

impl std::fmt::Display for ClockSkew {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "remote HLC wall_nanos {} is more than max_offset {}ns ahead of local wall_nanos {}",
            self.remote_wall_nanos, self.max_offset_nanos, self.local_wall_nanos
        )
    }
}
impl std::error::Error for ClockSkew {}

#[derive(Debug, Clone, Copy)]
struct HlcState {
    wall_nanos: u64,
    logical: u32,
    synthetic: bool,
    /// 最後に `now_ordinal` / `observe_ordinal` が発行した u64 射影値。
    /// u64 経路の厳密単調を保証するためのクランプ基準(案A→案B 射影の
    /// ロスで逆転しかけたら `last_ordinal + 1` を返す)。
    last_ordinal: u64,
}

/// `Hlc` 本体。`(wall_nanos, logical, synthetic, last_ordinal)` を
/// `parking_lot::Mutex` で保護する(CockroachDB `hlc.Clock` の `sync.Mutex`、
/// uhlc-rs の `Mutex` と同じ設計)。クリティカルセクションは数命令で、
/// 競合下でも実測上の問題は出ない。
pub struct Hlc {
    state: Mutex<HlcState>,
    /// クロックスキュー上限(ナノ秒、CockroachDB の `max_offset` 相当、
    /// 既定 500ms を推奨)。`0` = 無効(リモート値を常に受理、従来挙動)。
    max_offset_nanos: AtomicU64,
}

impl Hlc {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(HlcState { wall_nanos: 0, logical: 0, synthetic: false, last_ordinal: 0 }),
            max_offset_nanos: AtomicU64::new(0),
        }
    }

    /// テスト・決定的シミュレーション用に初期値を指定して構築する。
    /// `wall_nanos` はフル精度のまま保持される(案B と違いバケット整列しない)。
    pub fn with_initial(wall_nanos: u64, logical: u32) -> Self {
        let h = Self::new();
        {
            let mut st = h.state.lock();
            st.wall_nanos = wall_nanos;
            st.logical = logical;
            st.last_ordinal = HlcTimestamp { wall_nanos, logical, synthetic: false }.as_ordinal();
        }
        h
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

    /// リモート物理成分が壁時計 + `max_offset` を超えていないか検査する
    /// (フル精度ナノ秒での正確な比較。案B のバケット丸めは無い)。
    fn skew_ok(&self, remote_wall_nanos: u64, wall_now_nanos: u64) -> Result<(), ClockSkew> {
        let max_offset = self.max_offset_nanos.load(Ordering::SeqCst);
        if max_offset == 0 {
            return Ok(());
        }
        if remote_wall_nanos > wall_now_nanos.saturating_add(max_offset) {
            Err(ClockSkew {
                remote_wall_nanos,
                local_wall_nanos: wall_now_nanos,
                max_offset_nanos: max_offset,
            })
        } else {
            Ok(())
        }
    }

    /// ローカルイベント(書き込み等)発生時の遷移(ロック保持中に呼ぶ)。
    /// CockroachDB `hlc.go` `Now()` とビット単位で同じ規則。**バケット無し**:
    ///
    /// - 物理クロックが 1ns でも進んだ(`wall_now > st.wall_nanos`)
    ///   → `wall_nanos = wall_now`、`logical = 0`、`synthetic = false`。
    /// - それ以外(物理が停まっている / 後退)
    ///   → `logical += 1`(同一ナノ秒内の因果前進)、`synthetic = true`。
    ///   `logical` が u32 飽和する現実にはあり得ないケースだけ `wall_nanos` を
    ///   1ns 進めて `logical = 0`(厳密単調を死守)。
    fn advance_locked(st: &mut HlcState, wall_now_nanos: u64) -> HlcTimestamp {
        if wall_now_nanos > st.wall_nanos {
            st.wall_nanos = wall_now_nanos;
            st.logical = 0;
        } else {
            let bumped = st.logical.saturating_add(1);
            if bumped == st.logical {
                st.wall_nanos = st.wall_nanos.saturating_add(1);
                st.logical = 0;
            } else {
                st.logical = bumped;
            }
        }
        st.synthetic = st.wall_nanos > wall_now_nanos;
        HlcTimestamp { wall_nanos: st.wall_nanos, logical: st.logical, synthetic: st.synthetic }
    }

    /// ローカルイベント発生時に呼ぶ(フル精度)。`wall_now_nanos` は呼び出し側
    /// が渡す(テスト容易性。実運用は [`Hlc::now_hlc_sys`] / [`Hlc::now_ordinal`])。
    pub fn now_hlc(&self, wall_now_nanos: u64) -> HlcTimestamp {
        let mut st = self.state.lock();
        let ts = Self::advance_locked(&mut st, wall_now_nanos);
        st.last_ordinal = st.last_ordinal.max(ts.as_ordinal());
        ts
    }

    /// 旧名の互換エイリアス([`Hlc::now_hlc`])。
    pub fn now(&self, wall_now_nanos: u64) -> HlcTimestamp {
        self.now_hlc(wall_now_nanos)
    }

    /// `SystemTime::now()` を内部で読んでフル精度 [`HlcTimestamp`] を返す。
    pub fn now_hlc_sys(&self) -> HlcTimestamp {
        self.now_hlc(sys_wall_nanos())
    }

    /// `SystemTime::now()` を内部で読んで **u64 ordinal**(案B 射影、厳密単調)を
    /// 返す実運用向けメソッド。既存の `closed_ts` 系呼び出し元はこれを使う。
    pub fn now_ordinal(&self) -> u64 {
        let wall = sys_wall_nanos();
        let mut st = self.state.lock();
        let ts = Self::advance_locked(&mut st, wall);
        let o = ts.as_ordinal().max(st.last_ordinal.saturating_add(1));
        st.last_ordinal = o;
        o
    }

    /// リモートから受信した HLC と自クロックを統合する(CockroachDB `hlc.go`
    /// `Update` と同じ規則)。物理成分は 3 者(ローカル / リモート / 壁時計)の
    /// 最大、論理成分は採用された側 +1(壁時計が単独最大なら 0)。
    pub fn update(&self, remote: HlcTimestamp, wall_now_nanos: u64) -> HlcTimestamp {
        // スキュー上限を超えたリモート値はクロックを汚染しないよう無視し、
        // ローカル進行だけ行う(`try_update` は代わりに Err を返す)。
        if self.skew_ok(remote.wall_nanos, wall_now_nanos).is_err() {
            tracing::warn!(
                remote_wall_nanos = remote.wall_nanos,
                local_wall_nanos = wall_now_nanos,
                max_offset_ns = self.max_offset_nanos.load(Ordering::SeqCst),
                "HLC update: remote timestamp exceeds max_offset; ignoring remote, advancing locally only"
            );
            return self.now_hlc(wall_now_nanos);
        }
        let mut st = self.state.lock();
        let max_pt = st.wall_nanos.max(remote.wall_nanos).max(wall_now_nanos);
        let new_logical = if max_pt == st.wall_nanos && max_pt == remote.wall_nanos {
            st.logical.max(remote.logical).saturating_add(1)
        } else if max_pt == st.wall_nanos {
            st.logical.saturating_add(1)
        } else if max_pt == remote.wall_nanos {
            remote.logical.saturating_add(1)
        } else {
            0
        };
        let candidate = (max_pt, new_logical);
        // 厳密単調を保証(想定外の入力でも後退させない)。
        let (new_wall, new_log) = if candidate <= (st.wall_nanos, st.logical) {
            (st.wall_nanos, st.logical)
        } else {
            candidate
        };
        st.wall_nanos = new_wall;
        st.logical = new_log;
        st.synthetic = new_wall > wall_now_nanos;
        let ts = HlcTimestamp { wall_nanos: new_wall, logical: new_log, synthetic: st.synthetic };
        st.last_ordinal = st.last_ordinal.max(ts.as_ordinal());
        ts
    }

    /// `update` のスキュー検査版。上限を超えたリモート値なら `Err(ClockSkew)`
    /// を返す(クロックは進めない)。
    pub fn try_update(
        &self,
        remote: HlcTimestamp,
        wall_now_nanos: u64,
    ) -> Result<HlcTimestamp, ClockSkew> {
        self.skew_ok(remote.wall_nanos, wall_now_nanos)?;
        Ok(self.update(remote, wall_now_nanos))
    }

    /// フル精度 [`HlcTimestamp`] を受信して `update` する(新 API)。
    pub fn observe_hlc(&self, remote: HlcTimestamp, wall_now_nanos: u64) -> HlcTimestamp {
        self.update(remote, wall_now_nanos)
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
        let st = self.state.lock();
        HlcTimestamp { wall_nanos: st.wall_nanos, logical: st.logical, synthetic: st.synthetic }
    }
}

fn sys_wall_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
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
    fn as_ordinal_never_overflows_and_keeps_full_precision() {
        // 2026 年相当の実 Unix ナノ秒。旧々実装は wall<<16 でここが panic/ラップした。
        let real_nanos: u64 = 1_760_000_000_000_000_000;
        let ts = HlcTimestamp::new(real_nanos, 5);
        // P-HLC-3d: シフトもマスクも無い。ordinal == wall_nanos + logical。
        assert_eq!(ts.as_ordinal(), real_nanos + 5);
        assert_eq!(HlcTimestamp::new(real_nanos, 0).as_ordinal(), real_nanos, "logical 0 -> ordinal is exactly the nanosecond");
    }

    #[test]
    fn hlc_timestamp_is_full_precision_nanoseconds_no_truncation() {
        // 案A の肝: wall_nanos は切り捨てられない。
        let hlc = Hlc::new();
        let odd = 1_760_000_000_000_000_123u64; // 下位ビットが立った値
        let ts = hlc.now_hlc(odd);
        assert_eq!(ts.wall_nanos, odd, "wall_nanos must keep every nanosecond");
        assert_eq!(ts.logical, 0);
    }

    #[test]
    fn now_resets_logical_whenever_the_physical_clock_moves_forward() {
        let hlc = Hlc::with_initial(0x10_0000, 5);
        let ts = hlc.now_hlc(0x10_0000 + 1); // 物理が 1ns でも進めば logical リセット
        assert_eq!(ts.wall_nanos, 0x10_0000 + 1, "full precision preserved");
        assert_eq!(ts.logical, 0);
        assert!(!ts.synthetic);
    }

    #[test]
    fn now_bumps_logical_only_when_the_physical_clock_does_not_advance() {
        let hlc = Hlc::with_initial(0x10_0000, 5);
        let ts = hlc.now_hlc(0x0F_0000); // 物理クロックが後退
        assert_eq!(ts.wall_nanos, 0x10_0000, "wall held");
        assert_eq!(ts.logical, 6);
        assert!(ts.synthetic, "advanced by logical only -> synthetic");
    }

    #[test]
    fn now_is_monotonic_across_repeated_calls_with_same_wall_time() {
        let hlc = Hlc::new();
        let a = hlc.now_hlc(1_000_000);
        let b = hlc.now_hlc(1_000_000);
        let c = hlc.now_hlc(1_000_000);
        assert!(a < b && b < c, "{a:?} < {b:?} < {c:?}");
    }

    #[test]
    fn synthetic_flag_is_set_only_when_wall_nanos_runs_ahead_of_the_physical_reading() {
        let hlc = Hlc::with_initial(5_000, 0);
        // 物理クロックが後退している(1_000 < 5_000)→ logical で前進 → synthetic。
        let behind = hlc.now_hlc(1_000);
        assert_eq!(behind.wall_nanos, 5_000);
        assert!(behind.synthetic, "wall_nanos (5000) > physical reading (1000) -> synthetic");
        // 物理クロックが同一 65µs バケット内で進んだ → synthetic ではない。
        let caught_up = hlc.now_hlc(5_500);
        assert_eq!(caught_up.wall_nanos, 5_500);
        assert!(!caught_up.synthetic);
    }

    #[test]
    fn now_ordinal_is_strictly_monotonic_even_when_the_projection_would_regress() {
        // 物理クロックが 1ns 進んで logical が 0 に戻ると as_ordinal 単体は
        // 前回値を下回り得る。Hlc::now_ordinal は last_ordinal でクランプ。
        let hlc = Hlc::with_initial(0x30_0000, 900);
        let mut prev = 0u64;
        for i in 0..2000u64 {
            let o = {
                let wall = 0x30_0000 + i;
                let mut st = hlc.state.lock();
                let ts = Hlc::advance_locked(&mut st, wall);
                let o = ts.as_ordinal().max(st.last_ordinal.saturating_add(1));
                st.last_ordinal = o;
                o
            };
            assert!(o > prev, "now_ordinal must be strictly increasing: {o} !> {prev}");
            prev = o;
        }
    }

    #[test]
    fn ordinal_projection_is_monotonic_when_the_clock_sticks_then_jumps_forward() {
        let hlc = Hlc::new();
        let mut prev = 0u64;
        for _ in 0..60_000 {
            let o = hlc.now_hlc(0x30_0000 + 7).as_ordinal(); // 物理据え置き → logical だけ増える
            assert!(o > prev);
            prev = o;
        }
        let after = hlc.now_hlc(0x40_0000).as_ordinal(); // 物理が大きく前進
        assert!(after > prev, "forward jump must not invert: {after} !> {prev}");
    }

    #[test]
    fn as_ordinal_folds_logical_into_nanosecond_space_and_stays_monotonic() {
        let hlc = Hlc::with_initial(0x50_0000, 0);
        let mut prev = 0u64;
        let mut last = 0u64;
        for _ in 0..70_000 {
            let ts = hlc.now_hlc(0x50_0000); // 物理据え置き → logical++ のみ
            let o = ts.as_ordinal();
            assert_eq!(o, 0x50_0000 + ts.logical as u64, "ordinal == wall_nanos + logical, nothing else");
            assert!(o > prev, "monotonic: {o} !> {prev}");
            prev = o;
            last = o;
        }
        assert_eq!(last, 0x50_0000 + 70_000, "70k stuck-clock events -> exactly +70000 ns");
    }

    #[test]
    fn update_adopts_remote_wall_when_remote_is_ahead() {
        let hlc = Hlc::with_initial(0x10_0000, 0);
        let remote = HlcTimestamp::new(0x50_0000, 3);
        let ts = hlc.update(remote, 0x10_0000);
        assert_eq!(ts.wall_nanos, 0x50_0000);
        assert_eq!(ts.logical, 4);
    }

    #[test]
    fn update_keeps_local_when_local_is_ahead_of_remote_and_wall() {
        let hlc = Hlc::with_initial(0x90_0000, 2);
        let remote = HlcTimestamp::new(0x50_0000, 9);
        let ts = hlc.update(remote, 0x10_0000);
        assert_eq!(ts.wall_nanos, 0x90_0000);
        assert_eq!(ts.logical, 3);
    }

    #[test]
    fn update_uses_wall_when_it_exceeds_both_and_marks_not_synthetic() {
        let hlc = Hlc::with_initial(0x10_0000, 5);
        let remote = HlcTimestamp::new(0x20_0000, 9);
        let ts = hlc.update(remote, 0x99_0000 + 123);
        assert_eq!(ts.wall_nanos, 0x99_0000 + 123, "full precision physical reading");
        assert_eq!(ts.logical, 0);
        assert!(!ts.synthetic, "wall clock is the max -> not synthetic");
    }

    #[test]
    fn update_result_is_never_less_than_prior_local_value() {
        let hlc = Hlc::with_initial(0x10_0000_0000, 0);
        let before = hlc.peek();
        let remote = HlcTimestamp::new(0x1_0000, 1);
        let after = hlc.update(remote, 0x1);
        assert!(after >= before, "{after:?} >= {before:?}");
    }

    #[test]
    fn observe_ordinal_pulls_local_clock_forward() {
        let hlc = Hlc::with_initial(0x10_0000, 0);
        let remote_ord = 0x80_0007u64;
        hlc.observe_ordinal(remote_ord, 0x10_0000);
        let next = hlc.now_hlc(0x10_0000).as_ordinal();
        assert!(next > remote_ord, "local clock must advance past an observed remote ordinal");
    }

    #[test]
    fn observe_hlc_full_precision_round_trip() {
        // 新 API: フル精度 HLC をそのまま受信 → wall_nanos が保たれる。
        let hlc = Hlc::with_initial(1_000, 0);
        let remote = HlcTimestamp::new(1_760_000_000_000_000_777, 4);
        let merged = hlc.observe_hlc(remote, 1_000);
        assert_eq!(merged.wall_nanos, 1_760_000_000_000_000_777);
        assert_eq!(merged.logical, 5);
    }

    #[test]
    fn uncertainty_upper_is_wall_plus_max_offset() {
        let ts = HlcTimestamp::new(1_000_000_000, 3);
        assert_eq!(ts.uncertainty_upper(500_000_000), 1_500_000_000);
        assert_eq!(HlcTimestamp::new(u64::MAX - 10, 0).uncertainty_upper(1_000), u64::MAX);
    }

    #[test]
    fn as_ordinal_preserves_ordering_between_timestamps() {
        let a = HlcTimestamp::new(0x10_0000, 0);
        let b = HlcTimestamp::new(0x10_0000, 1);
        let c = HlcTimestamp::new(0x20_0000, 0);
        assert!(a.as_ordinal() < b.as_ordinal());
        assert!(b.as_ordinal() < c.as_ordinal());
    }

    #[test]
    fn eq_and_ord_ignore_the_synthetic_flag() {
        let real = HlcTimestamp { wall_nanos: 42, logical: 7, synthetic: false };
        let synth = HlcTimestamp { wall_nanos: 42, logical: 7, synthetic: true };
        assert_eq!(real, synth);
        assert_eq!(real.cmp(&synth), std::cmp::Ordering::Equal);
    }

    #[test]
    fn max_offset_rejects_a_remote_timestamp_from_the_far_future() {
        // 上限 1ms。壁時計は ~1ms、リモートは ~2.4 秒先。
        let hlc = Hlc::with_max_offset_nanos(1_000_000);
        let wall = 0x10_0000u64; // ~1.05e6 ns
        let far_future = HlcTimestamp::new(0x9000_0000, 3); // ~2.4e9 ns

        let err = hlc.try_update(far_future, wall).expect_err("should reject far-future remote");
        assert_eq!(err.max_offset_nanos, 1_000_000);
        assert_eq!(err.remote_wall_nanos, 0x9000_0000);

        // クロックは汚染されない: permissive update もローカル進行のみ。
        let local = hlc.update(far_future, wall);
        assert!(local.wall_nanos < 0x9000_0000, "clock not poisoned by rejected remote");

        // 上限内のリモートは通常どおり受理。
        let near = HlcTimestamp::new(wall + 500_000, 1);
        assert!(hlc.try_update(near, wall).is_ok());
    }

    #[test]
    fn max_offset_zero_disables_the_check_backward_compatible() {
        let hlc = Hlc::new();
        assert_eq!(hlc.max_offset_nanos(), 0);
        let far = HlcTimestamp::new(0x9000_0000, 0);
        assert!(hlc.try_update(far, 0x10_0000).is_ok(), "disabled check accepts anything");
        assert_eq!(hlc.peek().wall_nanos, 0x9000_0000);
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
                    out.push(hlc.now_ordinal());
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
