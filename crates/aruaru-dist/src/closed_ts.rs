//! Closed Timestamp と Follower Read (CockroachDB / TiKV safe-ts / YugabyteDB 方式)
//!
//! **背景 (2026-08-21)**: 「Snowflake のストレージ/コンピュート分離」と
//! 「CockroachDB の Raft 強整合」を両方持つ実在DBを再調査した結果、
//! *読み取りをリーダー(leaseholder)以外のレプリカへ逃がす仕組み*が、
//! この2つを実際に両立させている共通の要素技術だと分かった:
//!
//! - CockroachDB: 各 Range が **closed timestamp** を持ち、「この時刻以下に
//!   新しい書き込みは今後一切現れない」ことを保証する。leaseholder が
//!   closed timestamp を継続的に前進させ、Raft ログ経由または
//!   **side transport** で follower へ通知する。follower は
//!   `read_ts <= closed_ts` を満たす読み取りだけを自力で応答できる
//!   (`docs/RFCS/20181227_follower_reads_implementation.md`、
//!   `docs/RFCS/20210519_bounded_staleness_reads.md`)。
//! - TiKV/TiDB: 各 peer が **safe-ts** を持ち、`read_ts <= safe-ts` なら
//!   その peer からローカルに Stale Read できる (resolved-ts は leader
//!   のみが保持する概念、safe-ts は peer ごとの概念)。
//! - YugabyteDB: `yb_follower_read_staleness_ms` (既定30秒) という
//!   **上限付き陳腐化 (bounded staleness)** を受け入れる形で follower から
//!   一貫スナップショット読み取りを行う。
//!
//! aruaru-db にはこれまで lease / closed timestamp / follower read に相当する
//! 概念が**一切存在しなかった** (`grep -rniE "lease|closed_timestamp|
//! follower_read|bounded_staleness"` が `crates/` 内で無関係な
//! GitHub Release 関連の語にしかヒットしなかった)。読み取りは常に
//! リーダーのローカル状態機械を直接見る前提であり、Snowflake 型に
//! 「計算だけを増やしたレプリカ/ephemeral pod」が安全に読める根拠が
//! 無かった。本モジュールはその根拠 (closed timestamp) を実装する。
//!
//! ## スコープと正直な簡略化点
//!
//! 1. 時刻は**呼び出し側が渡す論理ナノ秒** (`Timestamp = u64`)。HLC
//!    (Hybrid Logical Clock) やクロックスキュー上限 (CockroachDB の
//!    `max_offset`) の管理は行わない——テストと管理APIからは単調増加する
//!    値を渡す。
//! 2. **MVCC 履歴読み取りそのものには接続していない**。本モジュールは
//!    「その時刻で読んでよいか」の判定 (安全性ゲート) までを担い、
//!    実際に過去バージョンを読み出す処理は既存の Git-on-SQL /
//!    `AS OF COMMIT` 経路の責務として分離している。
//! 3. side transport は**同一プロセス内のオブジェクト間通知**
//!    (`publish_to`) として実装する。ネットワーク越しの定期配送
//!    (CockroachDB の `closedts/sidetransport`) は、`aruaru-dist` の
//!    Raft トランスポートが真のネットワーク実装へ移行する時点で
//!    あわせて配線する。
//! 4. Range を跨ぐ読み取りの交渉は「関与する全 Range の closed timestamp
//!    の最小値」を取る単純方式。CockroachDB の bounded staleness が行う
//!    「ロックを避ける時刻交渉」までは行わない。

use parking_lot::RwLock;
use std::collections::{BTreeMap, HashMap};

/// 論理ナノ秒タイムスタンプ
pub type Timestamp = u64;

/// closed timestamp が現在時刻から遅れる目標間隔 (既定3秒)。
/// CockroachDB の `kv.closed_timestamp.target_duration` (既定3秒) に倣う。
pub const DEFAULT_TARGET_LAG_NANOS: u64 = 3_000_000_000;

/// follower read の既定上限陳腐化 (30秒)。
/// YugabyteDB の `yb_follower_read_staleness_ms` 既定30秒に倣う。
pub const DEFAULT_MAX_STALENESS_NANOS: u64 = 30_000_000_000;

/// 読み取り経路の決定結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadPlan {
    /// 任意のレプリカ (follower / learner / ephemeral な読み取り専用計算ノード)
    /// が自力で応答してよい。`timestamp` 以下の書き込みはすべて確定済み。
    FollowerRead {
        timestamp: Timestamp,
        /// `now - timestamp`
        staleness_nanos: u64,
    },
    /// closed timestamp では保証できないため、leaseholder (リーダー) へ
    /// ルーティングして強整合読み取りを行う必要がある。
    RouteToLeaseholder { reason: &'static str },
}

impl ReadPlan {
    pub fn is_follower_read(&self) -> bool {
        matches!(self, ReadPlan::FollowerRead { .. })
    }
    pub fn timestamp(&self) -> Option<Timestamp> {
        match self {
            ReadPlan::FollowerRead { timestamp, .. } => Some(*timestamp),
            ReadPlan::RouteToLeaseholder { .. } => None,
        }
    }
}

/// 単一 Range (= 単一 Raft グループ) の closed timestamp 追跡器。
///
/// leaseholder 側では `begin_write`/`end_write` で進行中の書き込み時刻を
/// 把握しつつ `advance_to(now)` で closed timestamp を前進させる。
/// follower 側では `receive_update` で leaseholder からの通知を取り込む。
pub struct ClosedTimestampTracker {
    closed: RwLock<Timestamp>,
    target_lag_nanos: u64,
    /// 進行中の書き込み: 書き込み時刻 → その時刻の未確定書き込み数。
    /// closed timestamp はこの最小値を**跨げない**——跨いだ瞬間に
    /// 「closed 以下に新しい書き込みは現れない」という保証が破れる。
    in_flight: RwLock<BTreeMap<Timestamp, u64>>,
}

impl ClosedTimestampTracker {
    pub fn new(target_lag_nanos: u64) -> Self {
        Self {
            closed: RwLock::new(0),
            target_lag_nanos,
            in_flight: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn with_default_lag() -> Self {
        Self::new(DEFAULT_TARGET_LAG_NANOS)
    }

    pub fn target_lag_nanos(&self) -> u64 {
        self.target_lag_nanos
    }

    pub fn closed_timestamp(&self) -> Timestamp {
        *self.closed.read()
    }

    /// 進行中書き込みの最小時刻 (無ければ None)。
    pub fn lowest_in_flight(&self) -> Option<Timestamp> {
        self.in_flight.read().keys().next().copied()
    }

    /// 書き込み開始 (提案時)。この時刻は確定するまで closed にできない。
    pub fn begin_write(&self, ts: Timestamp) {
        *self.in_flight.write().entry(ts).or_insert(0) += 1;
    }

    /// 書き込み完了 (commit + apply 完了時)。
    pub fn end_write(&self, ts: Timestamp) {
        let mut m = self.in_flight.write();
        if let Some(c) = m.get_mut(&ts) {
            *c -= 1;
            if *c == 0 {
                m.remove(&ts);
            }
        }
    }

    /// leaseholder 側: closed timestamp を `now - target_lag` まで前進させる。
    /// ただし進行中書き込みの最小時刻を跨がない。単調増加のみ (後退しない)。
    /// 戻り値は前進後の closed timestamp。
    pub fn advance_to(&self, now: Timestamp) -> Timestamp {
        let target = now.saturating_sub(self.target_lag_nanos);
        let bound = match self.lowest_in_flight() {
            // 進行中書き込みと同時刻は閉じられない (その時刻の直前まで)
            Some(low) => target.min(low.saturating_sub(1)),
            None => target,
        };
        let mut closed = self.closed.write();
        if bound > *closed {
            *closed = bound;
        }
        *closed
    }

    /// follower 側: leaseholder からの closed timestamp 通知を取り込む。
    /// 後退する通知 (再送・順序入替) は無視する。取り込んだら true。
    pub fn receive_update(&self, leader_closed: Timestamp) -> bool {
        let mut closed = self.closed.write();
        if leader_closed > *closed {
            *closed = leader_closed;
            true
        } else {
            false
        }
    }

    /// この時刻での読み取りをこのレプリカが自力で応答してよいか
    /// (TiKV の `read_ts <= safe-ts` と同じ判定)。
    pub fn can_serve_read_at(&self, read_ts: Timestamp) -> bool {
        read_ts != 0 && read_ts <= self.closed_timestamp()
    }
}

/// 複数 Range の closed timestamp を束ね、Range 横断の読み取り時刻交渉と
/// side transport による follower への配布を行うコーディネータ。
pub struct ClosedTimestampCoordinator {
    target_lag_nanos: u64,
    trackers: RwLock<HashMap<u64, std::sync::Arc<ClosedTimestampTracker>>>,
}

impl ClosedTimestampCoordinator {
    pub fn new(target_lag_nanos: u64) -> Self {
        Self { target_lag_nanos, trackers: RwLock::new(HashMap::new()) }
    }

    pub fn with_default_lag() -> Self {
        Self::new(DEFAULT_TARGET_LAG_NANOS)
    }

    /// Range を登録する (既にあれば既存のものを返す)。
    pub fn register_range(&self, range_id: u64) -> std::sync::Arc<ClosedTimestampTracker> {
        let mut t = self.trackers.write();
        t.entry(range_id)
            .or_insert_with(|| std::sync::Arc::new(ClosedTimestampTracker::new(self.target_lag_nanos)))
            .clone()
    }

    pub fn forget_range(&self, range_id: u64) {
        self.trackers.write().remove(&range_id);
    }

    pub fn tracker(&self, range_id: u64) -> Option<std::sync::Arc<ClosedTimestampTracker>> {
        self.trackers.read().get(&range_id).cloned()
    }

    pub fn range_ids(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self.trackers.read().keys().copied().collect();
        v.sort_unstable();
        v
    }

    /// leaseholder 側: 全 Range の closed timestamp を前進させ、
    /// `(range_id, closed_ts)` を range_id 昇順で返す。
    pub fn advance_all(&self, now: Timestamp) -> Vec<(u64, Timestamp)> {
        let trackers = self.trackers.read();
        let mut ids: Vec<u64> = trackers.keys().copied().collect();
        ids.sort_unstable();
        ids.into_iter()
            .filter_map(|id| trackers.get(&id).map(|t| (id, t.advance_to(now))))
            .collect()
    }

    /// side transport: 自分 (leaseholder 側) の closed timestamp を
    /// follower 側のコーディネータへ配布する。follower 側に未知の Range は
    /// その場で登録する。戻り値は実際に前進した Range 数。
    pub fn publish_to(&self, follower: &ClosedTimestampCoordinator) -> usize {
        let updates: Vec<(u64, Timestamp)> = {
            let trackers = self.trackers.read();
            trackers.iter().map(|(id, t)| (*id, t.closed_timestamp())).collect()
        };
        let mut advanced = 0;
        for (range_id, ts) in updates {
            let tracker = follower.register_range(range_id);
            if tracker.receive_update(ts) {
                advanced += 1;
            }
        }
        advanced
    }

    /// bounded staleness 交渉 (CockroachDB の bounded staleness read /
    /// YugabyteDB の `yb_follower_read_staleness_ms` に相当)。
    ///
    /// 関与する全 Range の closed timestamp の**最小値**を読み取り時刻として
    /// 採用する。その時刻が `max_staleness_nanos` の範囲内なら follower read
    /// を許可し、そうでなければ leaseholder へルーティングする。
    pub fn negotiate_bounded_staleness(
        &self,
        range_ids: &[u64],
        now: Timestamp,
        max_staleness_nanos: u64,
    ) -> ReadPlan {
        if range_ids.is_empty() {
            return ReadPlan::RouteToLeaseholder { reason: "no range specified" };
        }
        let trackers = self.trackers.read();
        let mut min_closed = Timestamp::MAX;
        for id in range_ids {
            match trackers.get(id) {
                Some(t) => min_closed = min_closed.min(t.closed_timestamp()),
                None => {
                    return ReadPlan::RouteToLeaseholder {
                        reason: "range has no closed timestamp on this replica",
                    }
                }
            }
        }
        if min_closed == 0 {
            return ReadPlan::RouteToLeaseholder { reason: "closed timestamp not yet advanced" };
        }
        let staleness = now.saturating_sub(min_closed);
        if staleness > max_staleness_nanos {
            return ReadPlan::RouteToLeaseholder { reason: "staleness bound exceeded" };
        }
        ReadPlan::FollowerRead { timestamp: min_closed, staleness_nanos: staleness }
    }

    /// exact staleness 読み取り (CockroachDB の
    /// `AS OF SYSTEM TIME follower_read_timestamp()` 相当)。
    /// 「`now - staleness` 時点で読む」と時刻を先に固定する方式で、
    /// 全 Range がその時刻を閉じ済みなら follower read を許可する。
    pub fn plan_exact_staleness_read(
        &self,
        range_ids: &[u64],
        now: Timestamp,
        staleness_nanos: u64,
    ) -> ReadPlan {
        if range_ids.is_empty() {
            return ReadPlan::RouteToLeaseholder { reason: "no range specified" };
        }
        let read_ts = now.saturating_sub(staleness_nanos);
        let trackers = self.trackers.read();
        for id in range_ids {
            match trackers.get(id) {
                Some(t) if t.can_serve_read_at(read_ts) => {}
                Some(_) => {
                    return ReadPlan::RouteToLeaseholder {
                        reason: "requested timestamp is above the closed timestamp",
                    }
                }
                None => {
                    return ReadPlan::RouteToLeaseholder {
                        reason: "range has no closed timestamp on this replica",
                    }
                }
            }
        }
        ReadPlan::FollowerRead { timestamp: read_ts, staleness_nanos }
    }
}

impl Default for ClosedTimestampCoordinator {
    fn default() -> Self {
        Self::with_default_lag()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: u64 = 1_000_000_000;

    #[test]
    fn closed_timestamp_advances_with_target_lag_and_never_regresses() {
        let t = ClosedTimestampTracker::new(3 * SEC);
        assert_eq!(t.closed_timestamp(), 0);
        assert_eq!(t.advance_to(10 * SEC), 7 * SEC);
        // 過去の now で呼ばれても後退しない
        assert_eq!(t.advance_to(5 * SEC), 7 * SEC);
        assert_eq!(t.advance_to(12 * SEC), 9 * SEC);
    }

    #[test]
    fn closed_timestamp_never_crosses_an_in_flight_write() {
        let t = ClosedTimestampTracker::new(3 * SEC);
        // 5秒時点の書き込みが進行中 -> closed は 5秒-1ns を超えられない
        t.begin_write(5 * SEC);
        assert_eq!(t.advance_to(10 * SEC), 5 * SEC - 1);
        // 進行中書き込みより下の読み取りは許されるが、その時刻自体は不可
        assert!(t.can_serve_read_at(5 * SEC - 1));
        assert!(!t.can_serve_read_at(5 * SEC));
        // 書き込み完了後は目標まで前進できる
        t.end_write(5 * SEC);
        assert_eq!(t.advance_to(10 * SEC), 7 * SEC);
        assert!(t.can_serve_read_at(5 * SEC));
    }

    #[test]
    fn read_at_zero_timestamp_is_never_served_by_a_follower() {
        let t = ClosedTimestampTracker::new(0);
        t.advance_to(10 * SEC);
        assert!(!t.can_serve_read_at(0));
    }

    #[test]
    fn follower_receives_closed_timestamp_via_side_transport_and_ignores_regressions() {
        let leader = ClosedTimestampCoordinator::new(3 * SEC);
        let follower = ClosedTimestampCoordinator::new(3 * SEC);
        leader.register_range(1);
        leader.register_range(2);
        leader.advance_all(10 * SEC);

        // follower は最初この Range を知らない -> side transport で登録+前進
        assert_eq!(leader.publish_to(&follower), 2);
        assert_eq!(follower.tracker(1).unwrap().closed_timestamp(), 7 * SEC);
        // 同じ通知を再送しても前進しない (冪等)
        assert_eq!(leader.publish_to(&follower), 0);
    }

    #[test]
    fn bounded_staleness_uses_the_minimum_closed_timestamp_across_ranges() {
        let c = ClosedTimestampCoordinator::new(3 * SEC);
        let r1 = c.register_range(1);
        let r2 = c.register_range(2);
        r1.advance_to(20 * SEC); // closed = 17s
        r2.advance_to(12 * SEC); // closed = 9s

        // 両 Range を読む -> 最小の 9s が採用される
        let plan = c.negotiate_bounded_staleness(&[1, 2], 20 * SEC, 30 * SEC);
        assert_eq!(plan, ReadPlan::FollowerRead { timestamp: 9 * SEC, staleness_nanos: 11 * SEC });
        // range1 のみなら 17s
        assert_eq!(
            c.negotiate_bounded_staleness(&[1], 20 * SEC, 30 * SEC).timestamp(),
            Some(17 * SEC)
        );
    }

    #[test]
    fn bounded_staleness_falls_back_to_leaseholder_when_bound_exceeded_or_unknown_range() {
        let c = ClosedTimestampCoordinator::new(3 * SEC);
        let r1 = c.register_range(1);
        r1.advance_to(10 * SEC); // closed = 7s

        // now=100s では 93秒の陳腐化 -> 上限5秒を超えるので leaseholder へ
        assert_eq!(
            c.negotiate_bounded_staleness(&[1], 100 * SEC, 5 * SEC),
            ReadPlan::RouteToLeaseholder { reason: "staleness bound exceeded" }
        );
        // 未知の Range
        assert!(matches!(
            c.negotiate_bounded_staleness(&[1, 99], 10 * SEC, 30 * SEC),
            ReadPlan::RouteToLeaseholder { .. }
        ));
        // Range 指定なし
        assert!(matches!(
            c.negotiate_bounded_staleness(&[], 10 * SEC, 30 * SEC),
            ReadPlan::RouteToLeaseholder { .. }
        ));
        // まだ前進していない Range
        c.register_range(5);
        assert_eq!(
            c.negotiate_bounded_staleness(&[5], 10 * SEC, 30 * SEC),
            ReadPlan::RouteToLeaseholder { reason: "closed timestamp not yet advanced" }
        );
    }

    #[test]
    fn exact_staleness_read_is_allowed_only_at_or_below_the_closed_timestamp() {
        let c = ClosedTimestampCoordinator::new(3 * SEC);
        let r1 = c.register_range(1);
        r1.advance_to(10 * SEC); // closed = 7s

        // now=10s, staleness=3s -> read_ts=7s == closed -> OK
        assert_eq!(
            c.plan_exact_staleness_read(&[1], 10 * SEC, 3 * SEC),
            ReadPlan::FollowerRead { timestamp: 7 * SEC, staleness_nanos: 3 * SEC }
        );
        // staleness=1s -> read_ts=9s > closed -> leaseholder へ
        assert_eq!(
            c.plan_exact_staleness_read(&[1], 10 * SEC, 1 * SEC),
            ReadPlan::RouteToLeaseholder {
                reason: "requested timestamp is above the closed timestamp"
            }
        );
    }
}
