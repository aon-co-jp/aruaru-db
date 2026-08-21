//! Raft 決定的シミュレーションテスト (DST: Deterministic Simulation Testing)
//!
//! FoundationDB が採用する手法(単一スレッド・完全決定的な擬似ネットワーク上で
//! 実際のプロダクションコードを走らせ、メッセージの遅延・欠落・重複・順序入替を
//! 注入して安全性を検証する)を、本リポジトリの自前 Raft 実装(`RaftNode`)向けに
//! 縮小移植したもの。
//!
//! 参考にした一次情報源(2026-08-21 多言語調査、WebSearchツールで実際に検索・確認):
//! - 日本語: https://apple.github.io/foundationdb/testing.html (アーキテクチャ文書、DST節)
//! - 英語: https://www.foundationdb.org/files/fdb-paper.pdf (FoundationDB論文、simulation節)
//! - 英語: https://antithesis.com/docs/resources/deterministic_simulation_testing/
//! - フランス語: https://pierrezemb.fr/posts/diving-into-foundationdb-simulation/
//! - 中国語: https://zhuanlan.zhihu.com/p/375321579 (FoundationDBのシミュレーションテスト解説)
//! - 韓国語: https://moonsub-kim.github.io/docs/distributed-systems/foundationdb/
//! - 英語(Rust実装事例): FOSDEM 2026 "Random seeds and state machines: An approach to
//!   deterministic simulation testing in Rust" https://fosdem.org/2026/schedule/event/GNTZDT-rust-deterministic-simulation-testing/
//!
//! ## このリポジトリでの位置づけ・正直な簡略化点
//!
//! FoundationDB本家のDSTは、実バイナリを丸ごと単一スレッドの離散イベント
//! シミュレータへ差し替え(ネットワーク/ディスク/クロックの全てを抽象化し、
//! BUGGIFYマクロでフォールトを注入)、本番コードを一切変更せずに検証する。
//! 本実装はそこまでの規模ではなく、以下の縮小版:
//!
//! 1. **対象は `RaftNode` の純粋な状態遷移ロジックのみ**(`append_entries` /
//!    `request_vote` / `maybe_commit` / `propose`)。実際のHTTPトランスポート
//!    (`HttpTransport`)・実プロセス起動・tokioランタイムは一切介さない
//!    (`RaftNode` のAPIが元々同期的・純粋関数に近いため、決定的シミュレーションに
//!    そのまま使い回せる設計になっている——これは元からの設計の利点であり、
//!    今回それを検証インフラとして初めて活用した)。
//! 2. **注入するフォールトはメッセージレベルのみ**: 欠落(drop)・重複(duplicate)・
//!    遅延並び替え(reorder、優先度キューで表現)。ディスク故障・プロセスクラッシュ・
//!    クロックスキューは注入しない(次回以降の課題)。
//! 3. **リーダー選挙は簡略化**: 本シミュレーションでは単一の固定リーダーを
//!    シナリオ開始時に決め打ちし、選挙アルゴリズム自体(`RaftDriver`の
//!    タイムアウト・Candidate昇格ロジック)は対象外。検証しているのは
//!    「メッセージが好き勝手な順序・タイミングで飛び交っても、コミット済みログが
//!    ノード間で決して食い違わない(Log Matching Property / State Machine Safety)」
//!    という複製安全性であり、これは前回HANDOFF(2026-08-21)で見つかった
//!    「AppendEntriesが認証ヘッダ無し・誤ったパスで送られ複製がサイレントに
//!    失敗する」ようなトランスポート層のバグは検出できない(役割分担が異なる)。
//! 4. 疑似乱数は外部crateへ依存せず、xorshift64を自前実装(シードのみで
//!    完全に再現可能な決定性を保証するため、依存クレートのバージョン差異による
//!    非決定性混入を避ける意図)。
//!
//! この規模でも実用的な価値がある: 単体テストが1回の呼び出し列しか検証しないのに
//! 対し、本シミュレーションは同じロジックを**何百通りものシード(=メッセージ順序)**
//! で反復実行し、"たまたま通っていた" レースを機械的に洗い出す。

use std::collections::BinaryHeap;
use std::sync::Mutex;

use super::command::{Command, CommandResponse};
use super::node::{AppendResult, Applier, RaftNode};

/// 依存クレート無しの決定的疑似乱数 (xorshift64star)。
/// シードが同じなら、同じマシン・同じ実行環境で常に同じ数列を返す。
pub struct SimRng(u64);

impl SimRng {
    pub fn new(seed: u64) -> Self {
        // 0 は不動点になるため避ける
        Self(if seed == 0 { 0x9E3779B97F4A7C15 } else { seed })
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    /// [0.0, 1.0) の一様乱数
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// [0, n) の整数
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next_u64() % n
        }
    }
}

/// フォールト注入の設定
#[derive(Debug, Clone)]
pub struct FaultConfig {
    /// メッセージが届かず失われる確率 [0.0, 1.0)
    pub drop_rate: f64,
    /// メッセージが複製されもう一通届く確率(ネットワーク層の再送重複を模す)
    pub duplicate_rate: f64,
    /// 遅延の最大ティック数(0〜max_delayでランダムに揺れる → 順序入替を誘発)
    pub max_delay_ticks: u64,
}

impl FaultConfig {
    pub fn none() -> Self {
        Self { drop_rate: 0.0, duplicate_rate: 0.0, max_delay_ticks: 0 }
    }
    pub fn chaotic() -> Self {
        Self { drop_rate: 0.2, duplicate_rate: 0.1, max_delay_ticks: 8 }
    }
}

/// シミュレーション用の記録 Applier (適用された Exec コマンドを順に記録する)
pub struct SimApplier {
    pub applied: Mutex<Vec<String>>,
}
impl SimApplier {
    pub fn new() -> Self {
        Self { applied: Mutex::new(Vec::new()) }
    }
}
impl Default for SimApplier {
    fn default() -> Self {
        Self::new()
    }
}
impl Applier for SimApplier {
    fn apply(&self, command: &Command) -> CommandResponse {
        if let Command::Exec(sql) = command {
            self.applied.lock().unwrap().push(sql.clone());
        }
        CommandResponse::ok()
    }
}

/// 遅延キューに積むイベント (AppendEntries の要求を模す。応答は簡略化のため
/// 同期的に即時処理し、Leader側の match_index 更新も同じ関数呼び出し内で行う——
/// 本シミュレーションの目的である「ログ複製の安全性」検証には、応答経路自体の
/// 非決定性は本質的でないため)。
struct Event {
    deliver_at: u64,
    /// 決定的な安定ソートのための連番(同じ deliver_at のイベントが投入順で
    /// 処理されるよう、BinaryHeapの比較に使う)
    seq: u64,
    to: usize,
    prev_log_index: u64,
    prev_log_term: u64,
    entries: Vec<super::LogEntry>,
    leader_commit: u64,
    term: u64,
}
impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        self.deliver_at == other.deliver_at && self.seq == other.seq
    }
}
impl Eq for Event {}
impl Ord for Event {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // BinaryHeap は max-heap なので、deliver_at が小さいものを先に取り出したい → 反転
        other
            .deliver_at
            .cmp(&self.deliver_at)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}
impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// 1回のシミュレーション実行結果
#[derive(Debug)]
pub struct SimReport {
    pub seed: u64,
    pub steps: u64,
    /// 各ノードの最終 commit_index
    pub commit_indices: Vec<u64>,
    /// 安全性違反(コミット済みログの食い違い)を検出したか
    pub safety_violation: Option<String>,
}

/// 決定的シミュレーションを1回実行する。
///
/// - `num_followers`: Leader (node 0 固定) 以外の Follower 数
/// - `num_commands`: Leader が提案する書き込みコマンド数
/// - `fault`: 注入するフォールトの強度
/// - `seed`: 疑似乱数シード(同じseedなら常に同じイベント順序 = 完全再現可能)
///
/// 戻り値の `safety_violation` が `Some` の場合、Raft の安全性
/// (Log Matching Property: 同じ index に同じ term のエントリがあれば、
/// それ以前の全エントリも一致する)が壊れていることを意味する——
/// このシード値を使えば同じ壊れ方を何度でも再現できる。
pub fn run_simulation(
    num_followers: usize,
    num_commands: u64,
    fault: &FaultConfig,
    seed: u64,
) -> SimReport {
    let mut rng = SimRng::new(seed);

    // node 0 = Leader、1..=num_followers = Follower。全員 voter (learnerはこの
    // シミュレーションの対象外——learner関連の安全性は node.rs の単体テスト
    // (test_learner_role_preserved_across_append_entries...等)で別途担保済み)。
    let peer_ids: Vec<u64> = (1..=num_followers as u64).collect();
    let leader: RaftNode<SimApplier> = RaftNode::new(0, SimApplier::new(), peer_ids.clone());
    leader.become_leader();
    let followers: Vec<RaftNode<SimApplier>> = (1..=num_followers)
        .map(|i| RaftNode::new(i as u64, SimApplier::new(), vec![]))
        .collect();

    let mut queue: BinaryHeap<Event> = BinaryHeap::new();
    let mut seq_counter: u64 = 0;
    let mut now: u64 = 0;

    // Leader がコマンドを提案するたびに、全 Follower へ AppendEntries イベントを
    // (フォールト注入つきで) 積む。
    for i in 0..num_commands {
        leader.propose(&Command::Exec(format!("INSERT {i}"))).unwrap();
        let (prev_index, prev_term, entries, leader_commit) = leader.build_append_for(1);
        // 簡略化: 全 follower へ同一内容 (prev_index はfollower0基準だが、本シミュレーションは
        // 全followerが同じ初期状態から出発するため妥当) を送る。
        for (fi, _) in followers.iter().enumerate() {
            maybe_enqueue(
                &mut queue,
                &mut seq_counter,
                &mut rng,
                fault,
                now,
                fi,
                prev_index,
                prev_term,
                entries.clone(),
                leader_commit,
                leader.term(),
            );
        }
        now += 1;
    }

    let mut steps = 0u64;
    // イベントキューを完全に消化するまで処理する(決定的な順序で)。
    while let Some(ev) = queue.pop() {
        steps += 1;
        let f = &followers[ev.to];
        let _: AppendResult = f.append_entries(
            ev.term,
            ev.prev_log_index,
            ev.prev_log_term,
            ev.entries,
            ev.leader_commit,
        );
        f.apply_committed();
        // Follower からの応答を模して Leader 側 match_index を進める
        // (本シミュレーションでは応答の欠落は安全性に影響しない——最悪でも
        // Leaderが複製の遅れを認識できず commit が遅延するだけで、安全性
        // 〈食い違い〉の検証対象ではないため、応答経路自体は決定的に処理する)。
        leader.update_match((ev.to + 1) as u64, f.last_index());
        leader.maybe_commit();
        leader.apply_committed();
    }

    // ── ハートビート段階 ──
    // 実際のRaftでは leader_commit は「次のAppendEntries(ハートビート含む)」に
    // 乗って初めてFollowerへ伝わる。上の複製フェーズでは各コマンド提案時点の
    // (まだ0のままの) leader_commit をエントリに埋め込んでいたため、Follower側は
    // ログの複製自体は終えていても commit_index が進まない。これを解消するため、
    // 実装同様「複製が進んだ後、追加のハートビートで leader_commit を配る」
    // 動作を複数ラウンド繰り返す(フォールトで一部が届かなくても、数ラウンド
    // 繰り返せば大半は伝播する——本物のRaftが定期ハートビートで同じことをする
    // のと同じ考え方)。
    const HEARTBEAT_ROUNDS: u64 = 5;
    for _round in 0..HEARTBEAT_ROUNDS {
        leader.maybe_commit();
        leader.apply_committed();
        let mut hb_queue: BinaryHeap<Event> = BinaryHeap::new();
        for (fi, _) in followers.iter().enumerate() {
            let peer_id = (fi + 1) as u64;
            let (prev_index, prev_term, _entries, _lc) = leader.build_append_for(peer_id);
            maybe_enqueue(
                &mut hb_queue,
                &mut seq_counter,
                &mut rng,
                fault,
                now,
                fi,
                prev_index,
                prev_term,
                Vec::new(), // ハートビートなので新規エントリは無し (leader_commit伝播のみ)
                leader.commit_index(),
                leader.term(),
            );
        }
        now += 1;
        while let Some(ev) = hb_queue.pop() {
            steps += 1;
            let f = &followers[ev.to];
            let _: AppendResult = f.append_entries(
                ev.term,
                ev.prev_log_index,
                ev.prev_log_term,
                ev.entries,
                ev.leader_commit,
            );
            f.apply_committed();
            leader.update_match((ev.to + 1) as u64, f.last_index());
        }
    }
    leader.maybe_commit();
    leader.apply_committed();

    // ── 安全性検証: commit 済みの範囲で、Leader と各 Follower のログが
    //    (index, term) の対で完全一致しているか ──
    let mut violation = None;
    'outer: for (fi, f) in followers.iter().enumerate() {
        let common_commit = leader.commit_index().min(f.commit_index());
        for idx in 1..=common_commit {
            let lt = leader_term_at(&leader, idx);
            let ft = leader_term_at(f, idx);
            if lt != ft {
                violation = Some(format!(
                    "node {} と leader が index={} で term不一致 (leader_term={:?}, follower_term={:?}) — Log Matching Property違反",
                    fi + 1,
                    idx,
                    lt,
                    ft
                ));
                break 'outer;
            }
        }
    }

    let mut commit_indices = vec![leader.commit_index()];
    commit_indices.extend(followers.iter().map(|f| f.commit_index()));

    SimReport { seed, steps, commit_indices, safety_violation: violation }
}

fn leader_term_at(n: &RaftNode<SimApplier>, idx: u64) -> Option<u64> {
    // `RaftNode::term_at_index` は本シミュレーション用に node.rs へ追加した
    // 薄い委譲メソッド(内部の `ReplicatedLog::term_at` をそのまま公開する)。
    n.term_at_index(idx)
}

#[allow(clippy::too_many_arguments)]
fn maybe_enqueue(
    queue: &mut BinaryHeap<Event>,
    seq_counter: &mut u64,
    rng: &mut SimRng,
    fault: &FaultConfig,
    now: u64,
    to: usize,
    prev_log_index: u64,
    prev_log_term: u64,
    entries: Vec<super::LogEntry>,
    leader_commit: u64,
    term: u64,
) {
    if fault.drop_rate > 0.0 && rng.next_f64() < fault.drop_rate {
        return; // 欠落
    }
    let delay = if fault.max_delay_ticks > 0 { rng.next_below(fault.max_delay_ticks + 1) } else { 0 };
    *seq_counter += 1;
    queue.push(Event {
        deliver_at: now + delay,
        seq: *seq_counter,
        to,
        prev_log_index,
        prev_log_term,
        entries: entries.clone(),
        leader_commit,
        term,
    });
    if fault.duplicate_rate > 0.0 && rng.next_f64() < fault.duplicate_rate {
        // 重複配送: 別の遅延で同じ内容をもう一通積む
        let delay2 = if fault.max_delay_ticks > 0 {
            rng.next_below(fault.max_delay_ticks + 1)
        } else {
            0
        };
        *seq_counter += 1;
        queue.push(Event {
            deliver_at: now + delay2,
            seq: *seq_counter,
            to,
            prev_log_index,
            prev_log_term,
            entries,
            leader_commit,
            term,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// フォールト無しの基準シナリオ: 全メッセージが即時・確実に届く場合、
    /// 全ノードが同じ commit_index に到達し、安全性違反も無い。
    #[test]
    fn test_sim_no_faults_converges() {
        let report = run_simulation(3, 20, &FaultConfig::none(), 42);
        assert!(report.safety_violation.is_none(), "{:?}", report.safety_violation);
        // フォールト無しなら全ノードが最終的に同じ commit_index (=20) に到達する
        assert!(report.commit_indices.iter().all(|&c| c == 20), "{:?}", report.commit_indices);
    }

    /// 【本命】欠落・重複・遅延並び替えを注入した混沌シナリオを、多数のシードで
    /// 反復実行する DST の中核テスト。FoundationDBが「一晩に数万回のシミュレーション」
    /// を回すのと同じ考え方を縮小適用: ここでは200シード(CIでの実行時間を考慮した
    /// 現実的な回数、正直な簡略化点)を回し、どのシードでも Log Matching Property が
    /// 破れないことを確認する。安全性が壊れた場合、失敗メッセージにそのシード値が
    /// 出力されるため、開発者はそのシードだけを指定して単発再実行し確定的に再現できる。
    #[test]
    fn test_sim_chaotic_many_seeds_never_violates_log_matching() {
        let fault = FaultConfig::chaotic();
        for seed in 1..=200u64 {
            let report = run_simulation(4, 30, &fault, seed);
            assert!(
                report.safety_violation.is_none(),
                "seed={} で安全性違反を検出: {:?} (このseedを直接 run_simulation に渡せば再現可能)",
                seed,
                report.safety_violation
            );
        }
    }

    /// 極端に高いdrop_rateでも(コミットが進まないことはあっても)安全性は壊れない
    /// ことを確認する境界ケース。
    #[test]
    fn test_sim_extreme_drop_rate_still_safe() {
        let fault = FaultConfig { drop_rate: 0.9, duplicate_rate: 0.3, max_delay_ticks: 15 };
        for seed in 1..=50u64 {
            let report = run_simulation(3, 15, &fault, seed);
            assert!(report.safety_violation.is_none(), "seed={} {:?}", seed, report.safety_violation);
        }
    }

    /// 同じシードは常に同じ結果を返す(=真に決定的である)ことを保証する回帰テスト。
    /// これが崩れると「バグを再現できるシード値」という DST の核心的価値が失われる。
    #[test]
    fn test_sim_same_seed_is_fully_deterministic() {
        let fault = FaultConfig::chaotic();
        let r1 = run_simulation(3, 25, &fault, 777);
        let r2 = run_simulation(3, 25, &fault, 777);
        assert_eq!(r1.steps, r2.steps);
        assert_eq!(r1.commit_indices, r2.commit_indices);
        assert_eq!(r1.safety_violation.is_none(), r2.safety_violation.is_none());
    }
}
