//! Raft 合意ランタイム (driver)
//!
//! tokio 上で動く合意ループ:
//! - Follower: 選挙タイムアウトで Candidate に昇格し RequestVote を送る
//! - Candidate: 過半数の票を得たら Leader に昇格
//! - Leader: 定期ハートビート + ログ複製 (AppendEntries) を送り、多数決で commit を進める
//! - commit が進んだら状態機械へ apply
//!
//! 選挙タイムアウトはノードごとにランダム化し、分割投票を避ける。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::node::{Applier, RaftNode};
use super::rpc::{AppendEntriesReq, RequestVoteReq};
use super::transport::Transport;
use super::RaftRole;

/// ハートビート間隔
const HEARTBEAT: Duration = Duration::from_millis(150);
/// 選挙タイムアウト下限/上限 (ランダム化)
const ELECTION_MIN_MS: u64 = 600;
const ELECTION_MAX_MS: u64 = 1200;

/// 合意ランタイム
pub struct RaftDriver<A: Applier, T: Transport> {
    node: Arc<RaftNode<A>>,
    transport: Arc<T>,
    /// 最後に Leader からの正当な接触を受けた論理時刻 (ms 累積)
    last_contact_ms: AtomicU64,
    clock_ms: AtomicU64,
}

impl<A: Applier + 'static, T: Transport + 'static> RaftDriver<A, T> {
    pub fn new(node: Arc<RaftNode<A>>, transport: Arc<T>) -> Arc<Self> {
        Arc::new(Self {
            node,
            transport,
            last_contact_ms: AtomicU64::new(0),
            clock_ms: AtomicU64::new(0),
        })
    }

    /// Leader からの接触を記録 (選挙タイマーをリセット)
    pub fn note_contact(&self) {
        let now = self.clock_ms.load(Ordering::Relaxed);
        self.last_contact_ms.store(now, Ordering::Relaxed);
    }

    /// ノードごとにランダム化した選挙タイムアウト
    fn election_timeout_ms(node_id: u64) -> u64 {
        // 乱数の代わりに node_id とプロセス時刻で散らす (依存を増やさない簡易版)
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0);
        let span = ELECTION_MAX_MS - ELECTION_MIN_MS;
        ELECTION_MIN_MS + ((nanos.wrapping_add(node_id.wrapping_mul(97))) % span)
    }

    /// メインループを起動する。tokio タスクとして spawn して常駐させる。
    pub async fn run(self: Arc<Self>) {
        let mut ticker = tokio::time::interval(HEARTBEAT);
        let mut timeout = Self::election_timeout_ms(self.node.node_id());

        loop {
            ticker.tick().await;
            let elapsed = HEARTBEAT.as_millis() as u64;
            let now = self.clock_ms.fetch_add(elapsed, Ordering::Relaxed) + elapsed;

            match self.node.role() {
                RaftRole::Leader => {
                    self.replicate().await;
                    self.node.maybe_commit();
                    self.node.apply_committed();
                    self.note_contact();
                }
                RaftRole::Follower | RaftRole::Candidate => {
                    let since = now.saturating_sub(self.last_contact_ms.load(Ordering::Relaxed));
                    if since >= timeout {
                        self.start_election().await;
                        // 次回タイムアウトを再ランダム化
                        timeout = Self::election_timeout_ms(self.node.node_id());
                        self.note_contact();
                    }
                    // 適用は常に進める
                    self.node.apply_committed();
                }
                RaftRole::Learner => {
                    self.node.apply_committed();
                }
            }
        }
    }

    /// Leader: 全 peer へ AppendEntries (ログ複製 + ハートビート)
    async fn replicate(&self) {
        let term = self.node.term();
        let leader_id = self.node.node_id();
        for &peer in self.node.peers() {
            let (prev_log_index, prev_log_term, entries, leader_commit) =
                self.node.build_append_for(peer);
            let req = AppendEntriesReq {
                term,
                leader_id,
                prev_log_index,
                prev_log_term,
                entries,
                leader_commit,
            };
            match self.transport.send_append_entries(peer, req).await {
                Ok(resp) => {
                    if resp.term > term {
                        // より新しい term を検知 → 退位
                        self.node.become_follower(resp.term);
                        return;
                    }
                    if resp.success {
                        self.node.update_match(peer, resp.match_index);
                    } else {
                        // 不一致: match を 1 つ巻き戻して次回再送 (nextIndex デクリメント相当)
                        let cur = self.node.match_of(peer);
                        self.node.update_match(peer, cur.saturating_sub(1));
                    }
                }
                Err(e) => {
                    tracing::debug!(peer, error = %e, "append_entries send failed");
                }
            }
        }
    }

    /// Candidate になり RequestVote を送って集票
    async fn start_election(&self) {
        let term = self.node.become_candidate();
        let me = self.node.node_id();
        let last_log_index = self.node.last_index();
        let last_log_term = self.node.last_log_term();
        tracing::info!(node = me, term, "starting election");

        let mut votes = 1usize; // 自分の票
        let cluster = self.node.peers().len() + 1;

        for &peer in self.node.peers() {
            let req = RequestVoteReq {
                term,
                candidate_id: me,
                last_log_index,
                last_log_term,
            };
            match self.transport.send_request_vote(peer, req).await {
                Ok(resp) => {
                    if resp.term > term {
                        self.node.become_follower(resp.term);
                        return;
                    }
                    if resp.vote_granted {
                        votes += 1;
                    }
                }
                Err(e) => tracing::debug!(peer, error = %e, "request_vote send failed"),
            }
        }

        // 過半数を獲得かつまだ Candidate なら Leader 昇格
        if votes > cluster / 2 && self.node.role() == RaftRole::Candidate {
            tracing::info!(node = me, term, votes, "won election → leader");
            self.node.become_leader();
            // Leader 確立直後は各 peer の match を 0 に初期化 (nextIndex = last+1 相当)
            for &peer in self.node.peers() {
                self.node.update_match(peer, 0);
            }
        }
    }

    pub fn node(&self) -> &Arc<RaftNode<A>> {
        &self.node
    }
}
