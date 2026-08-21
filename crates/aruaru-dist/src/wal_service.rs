//! WAL サービス (safekeeper) と Pageserver の分離 — Neon 方式
//!
//! **背景 (2026-08-22)**: 前回セッション (CLAUDE.md の 2026-08-21(続き6)
//! エントリ) で「次回の調査目星」として残した1件目、**Neon の
//! pageserver / safekeeper 分離**を一次資料で確認し、aruaru-db に
//! 相当物が無いことをコードで裏取りした上で実装したもの。
//!
//! ## 一次資料で確認した Neon の設計
//!
//! - `neondatabase/neon` の [`docs/walservice.md`] によれば、compute が
//!   生成した WAL は複数の **safekeeper** へストリームされ、
//!   「**過半数 (majority) の safekeeper がローカルディスクへ書き終えた
//!   時点で durable** と見なす」。safekeeper 群は Paxos ベースの合意で
//!   WAL を多重化し、**単一 primary の強制** (2つの compute が同時に
//!   書くことを防ぐ) もこの層が担う。pageserver は primary からでは
//!   なく **safekeeper 群から** streaming replication で WAL を引く。
//! - [`docs/safekeeper-protocol.md`] によれば、proposer は
//!   `(term, UUID)` の NodeID を持ち、**term は proposer 起動ごとに
//!   増加**して split-brain を防ぐ。safekeeper は自分が受理した
//!   NodeID 以上の提案のみ受理する (それ未満は fence される)。
//!   `commitLSN` は「全 safekeeper の `flushLSN` を並べた配列の
//!   `flushLsn[n - quorum]` 要素」——すなわち **quorum 番目に大きい
//!   flushLSN** として計算される。
//! - pageserver 側 ([`docs/pageserver-storage.md`]、ブログ
//!   "Deep dive into Neon storage engine") は WAL を継続的に取り込み、
//!   リレーション/ページ単位に切り分けて **要求された LSN のページを
//!   base image + delta から再構成**する (`get_page_at_lsn`)。
//!   そして「対応する WAL が届くまでページ要求に応答しない」ことで
//!   一貫性を保証し、`max_replication_*_lag` によるバックプレッシャで
//!   遅延を抑える。
//!
//! [`docs/walservice.md`]: https://github.com/neondatabase/neon/blob/main/docs/walservice.md
//! [`docs/safekeeper-protocol.md`]: https://github.com/neondatabase/neon/blob/main/docs/safekeeper-protocol.md
//! [`docs/pageserver-storage.md`]: https://github.com/neondatabase/neon/blob/main/docs/pageserver-storage.md
//!
//! ## コードで裏取りしたギャップ
//!
//! `grep -rniE "safekeeper|pageserver|wal_service|commit_lsn|lsn" crates`
//! を実行した結果、`crates/` 内で LSN に言及していたのは
//! `aruaru-core/src/version/mod.rs` の `create_branch_from`
//! (Neon 方式ブランチングの**コメント**) だけで、**WAL を独立した
//! quorum で永続化する層、および「LSN 指定でページを再構成する層」は
//! 一切存在しなかった**。既存の `aruaru-dist::raft` は
//! 「合意 + 状態機械への適用」を同一ノード内で一体に行う構成であり、
//! 「WAL の耐久化 (safekeeper) と ページ再構成 (pageserver) を別プロセス
//! ・別ハードウェア特性へ分離する」という Neon の中核設計は無かった。
//! 本モジュールはその分離をこのリポジトリ内に実装する。
//!
//! ## スコープと正直な簡略化点 (誇張しない)
//!
//! 1. **同一プロセス内のオブジェクト分離**であり、ネットワーク越しの
//!    streaming replication は未実装 (`aruaru-dist::raft::transport`
//!    が真のネットワーク実装へ移る時点で併せて配線する)。
//! 2. Paxos の完全実装ではない。**term による fencing と quorum flushLSN
//!    による commitLSN 決定**という、単一 primary 強制と durability の
//!    核だけを実装する (投票フェーズでの WAL 突き合わせ・term_history の
//!    復旧は未実装)。
//! 3. ストレージは**メモリ上** (`BTreeMap`)。Neon の layer file / S3
//!    アップロードに相当する永続化は未実装 (既存の `aruaru-backup` との
//!    接続は次回課題)。
//! 4. `PageDelta` は `Replace` / `Append` の2種のみの単純なモデルで、
//!    PostgreSQL の WAL レコード再生 (`redo`) ではない。
//! 5. 既存の SQL 実行経路・`AS OF COMMIT` 読み取りには**未接続**。
//!    本モジュール単体で完結する層として追加している。

use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Log Sequence Number。WAL 上の位置を表す単調増加値。
pub type Lsn = u64;

/// proposer (書き込み側 compute) の世代番号。起動ごとに増える。
pub type Term = u64;

/// pageserver が commitLSN からどれだけ遅れてよいかの既定上限。
/// Neon の `max_replication_apply_lag` 等のバックプレッシャに相当する
/// (Neon は byte 単位、ここは LSN 単位)。
pub const DEFAULT_MAX_REPLICATION_LAG: u64 = 1024;

/// ページに対する差分操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageDelta {
    /// ページ全体を置き換える (Neon の image layer 相当の起点になる)
    Replace(Vec<u8>),
    /// 末尾へ追記する
    Append(Vec<u8>),
}

/// WAL レコード 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalRecord {
    pub lsn: Lsn,
    pub page_key: String,
    pub delta: PageDelta,
}

impl WalRecord {
    pub fn replace(lsn: Lsn, page_key: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self { lsn, page_key: page_key.into(), delta: PageDelta::Replace(bytes.into()) }
    }
    pub fn append(lsn: Lsn, page_key: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self { lsn, page_key: page_key.into(), delta: PageDelta::Append(bytes.into()) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WalServiceError {
    /// より新しい proposer (term) に追い越されたため受理を拒否された
    /// (Neon の「単一 primary 強制」= split-brain 防止)。
    #[error("proposer term {proposed} is fenced by accepted term {accepted}")]
    Fenced { proposed: Term, accepted: Term },
    /// quorum の safekeeper から ack が得られなかった
    #[error("only {acked} of {needed} required safekeepers accepted the WAL")]
    QuorumNotReached { acked: usize, needed: usize },
    /// pageserver の取り込みが commitLSN から離れすぎている
    #[error("replication lag {lag} exceeds limit {limit}")]
    LagTooLarge { lag: u64, limit: u64 },
    /// 要求 LSN の WAL が pageserver へまだ届いていない
    /// (Neon の「WAL が来るまでページ要求に応答しない」)
    #[error("requested lsn {requested} is ahead of last_record_lsn {last_record_lsn}")]
    WalNotArrived { requested: Lsn, last_record_lsn: Lsn },
    /// LSN が単調増加していない
    #[error("lsn {lsn} is not greater than flush_lsn {flush_lsn}")]
    NonMonotonicLsn { lsn: Lsn, flush_lsn: Lsn },
    /// 指定ページが存在しない
    #[error("page {0} not found at the requested lsn")]
    PageNotFound(String),
    /// image layer 生成 (compaction) により、その LSN 未満の delta が
    /// GC 済みで再構成できない (Neon の GC cutoff / `pitr_interval` 相当)。
    #[error("lsn {requested} is below the gc cutoff (image layer at {image_lsn})")]
    BelowGcCutoff { requested: Lsn, image_lsn: Lsn },
}

/// safekeeper 1 台。WAL を受理してローカルへ「flush」し、`flush_lsn` を進める。
#[derive(Debug)]
pub struct Safekeeper {
    id: u64,
    /// 受理済みの最大 term (これ未満の proposer は fence する)
    accepted_term: RwLock<Term>,
    /// ローカルへ flush 済みの WAL
    wal: RwLock<BTreeMap<Lsn, WalRecord>>,
}

impl Safekeeper {
    pub fn new(id: u64) -> Self {
        Self { id, accepted_term: RwLock::new(0), wal: RwLock::new(BTreeMap::new()) }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn accepted_term(&self) -> Term {
        *self.accepted_term.read()
    }

    /// ディスクへ flush 済みの WAL の末端 (無ければ 0)。
    pub fn flush_lsn(&self) -> Lsn {
        self.wal.read().keys().next_back().copied().unwrap_or(0)
    }

    /// proposer の挨拶 (AcceptorGreeting 相当)。`term` が受理済み term
    /// 以上なら受理して自分の term を更新し、`(term, flush_lsn)` を返す。
    pub fn greet(&self, term: Term) -> Result<(Term, Lsn), WalServiceError> {
        let mut accepted = self.accepted_term.write();
        if term < *accepted {
            return Err(WalServiceError::Fenced { proposed: term, accepted: *accepted });
        }
        *accepted = term;
        Ok((term, self.flush_lsn()))
    }

    /// WAL を受理して flush する。`term` が受理済み term 未満なら fence。
    pub fn accept(&self, term: Term, records: &[WalRecord]) -> Result<Lsn, WalServiceError> {
        {
            let accepted = self.accepted_term.read();
            if term < *accepted {
                return Err(WalServiceError::Fenced { proposed: term, accepted: *accepted });
            }
        }
        let mut wal = self.wal.write();
        let mut flush = wal.keys().next_back().copied().unwrap_or(0);
        for r in records {
            if r.lsn <= flush {
                return Err(WalServiceError::NonMonotonicLsn { lsn: r.lsn, flush_lsn: flush });
            }
            flush = r.lsn;
            wal.insert(r.lsn, r.clone());
        }
        Ok(flush)
    }

    /// `after` より大きく `up_to` 以下の WAL を返す (pageserver の
    /// streaming replication 相当)。
    pub fn stream(&self, after: Lsn, up_to: Lsn) -> Vec<WalRecord> {
        self.wal
            .read()
            .range((std::ops::Bound::Excluded(after), std::ops::Bound::Included(up_to)))
            .map(|(_, r)| r.clone())
            .collect()
    }

    /// pageserver が取り込み済みの WAL を解放する
    /// (Neon の safekeeper は「一時的な耐障害ストレージ」であり、
    /// S3 へ落ちた分は捨てられる)。解放件数を返す。
    pub fn truncate_up_to(&self, lsn: Lsn) -> usize {
        let mut wal = self.wal.write();
        let keep: BTreeMap<Lsn, WalRecord> =
            wal.split_off(&(lsn.saturating_add(1)));
        let removed = wal.len();
        *wal = keep;
        removed
    }
}

/// safekeeper 群 (WAL サービス)。proposer 側の commitLSN 計算を担う。
#[derive(Debug)]
pub struct WalService {
    safekeepers: Vec<Arc<Safekeeper>>,
    /// 現在の proposer の term (0 = proposer 未起動)
    term: RwLock<Term>,
    commit_lsn: RwLock<Lsn>,
}

impl WalService {
    /// `n` 台構成の WAL サービスを作る。
    pub fn with_safekeepers(n: usize) -> Self {
        assert!(n >= 1, "at least one safekeeper is required");
        Self {
            safekeepers: (1..=n as u64).map(|id| Arc::new(Safekeeper::new(id))).collect(),
            term: RwLock::new(0),
            commit_lsn: RwLock::new(0),
        }
    }

    pub fn len(&self) -> usize {
        self.safekeepers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.safekeepers.is_empty()
    }

    /// 過半数 (majority)。
    pub fn quorum(&self) -> usize {
        self.safekeepers.len() / 2 + 1
    }

    pub fn safekeeper(&self, id: u64) -> Option<Arc<Safekeeper>> {
        self.safekeepers.iter().find(|s| s.id() == id).cloned()
    }

    pub fn term(&self) -> Term {
        *self.term.read()
    }

    /// quorum が受理した位置 = 読み取って安全な WAL 末端。
    pub fn commit_lsn(&self) -> Lsn {
        *self.commit_lsn.read()
    }

    /// 新しい proposer (書き込み側 compute) を起動する。既存の最大 term + 1
    /// を全 safekeeper へ通知し、**古い proposer を fence** する
    /// (Neon の「term は proposer 起動ごとに増加して split-brain を防ぐ」)。
    /// 返り値は新しい term。
    pub fn start_proposer(&self) -> Term {
        let mut term = self.term.write();
        let highest = self
            .safekeepers
            .iter()
            .map(|s| s.accepted_term())
            .max()
            .unwrap_or(0)
            .max(*term);
        let new_term = highest + 1;
        for s in &self.safekeepers {
            // greet は `term >= accepted` なら必ず成功する
            let _ = s.greet(new_term);
        }
        *term = new_term;
        new_term
    }

    /// WAL を全 safekeeper へ送り、quorum 番目に大きい `flush_lsn` を
    /// commitLSN として採用する (`flushLsn[n - quorum]`、
    /// `docs/safekeeper-protocol.md`)。
    pub fn append(&self, term: Term, records: &[WalRecord]) -> Result<Lsn, WalServiceError> {
        let mut acked = 0usize;
        let mut fenced_by: Option<Term> = None;
        for s in &self.safekeepers {
            match s.accept(term, records) {
                Ok(_) => acked += 1,
                Err(WalServiceError::Fenced { accepted, .. }) => {
                    fenced_by = Some(fenced_by.map_or(accepted, |t: Term| t.max(accepted)));
                }
                Err(_) => {}
            }
        }
        let needed = self.quorum();
        if acked < needed {
            if let Some(accepted) = fenced_by {
                return Err(WalServiceError::Fenced { proposed: term, accepted });
            }
            return Err(WalServiceError::QuorumNotReached { acked, needed });
        }
        Ok(self.recompute_commit_lsn())
    }

    /// 全 safekeeper の flush_lsn を降順に並べ、`quorum` 番目の値を
    /// commitLSN とする。単調増加のみ。
    pub fn recompute_commit_lsn(&self) -> Lsn {
        let mut flushed: Vec<Lsn> = self.safekeepers.iter().map(|s| s.flush_lsn()).collect();
        flushed.sort_unstable_by(|a, b| b.cmp(a));
        let candidate = flushed[self.quorum() - 1];
        let mut commit = self.commit_lsn.write();
        if candidate > *commit {
            *commit = candidate;
        }
        *commit
    }

    /// `after` より大きく commitLSN 以下の WAL を、その範囲を持っている
    /// safekeeper から取得する (pageserver は primary ではなく
    /// safekeeper 群から WAL を引く)。
    pub fn stream_committed(&self, after: Lsn) -> Vec<WalRecord> {
        let commit = self.commit_lsn();
        for s in &self.safekeepers {
            if s.flush_lsn() >= commit {
                let batch = s.stream(after, commit);
                if !batch.is_empty() || after >= commit {
                    return batch;
                }
            }
        }
        Vec::new()
    }
}

/// ページ 1 枚の再構成材料 (Neon の image layer + delta layer 相当)。
#[derive(Debug, Default, Clone)]
struct PageHistory {
    /// materialize 済みのスナップショット (image layer)。`(lsn, bytes)`
    image: Option<(Lsn, Vec<u8>)>,
    /// image より後の差分 (delta layer)
    deltas: BTreeMap<Lsn, PageDelta>,
}

/// Pageserver。safekeeper 群から WAL を取り込み、
/// **任意の LSN 時点のページを再構成**して返す。
#[derive(Debug)]
pub struct Pageserver {
    pages: RwLock<BTreeMap<String, PageHistory>>,
    /// 取り込み済み WAL の末端
    last_record_lsn: RwLock<Lsn>,
    max_replication_lag: u64,
}

impl Pageserver {
    pub fn new(max_replication_lag: u64) -> Self {
        Self {
            pages: RwLock::new(BTreeMap::new()),
            last_record_lsn: RwLock::new(0),
            max_replication_lag,
        }
    }

    pub fn with_default_lag_limit() -> Self {
        Self::new(DEFAULT_MAX_REPLICATION_LAG)
    }

    pub fn last_record_lsn(&self) -> Lsn {
        *self.last_record_lsn.read()
    }

    pub fn max_replication_lag(&self) -> u64 {
        self.max_replication_lag
    }

    /// WAL サービスから commitLSN までを取り込む。取り込んだ件数を返す。
    pub fn ingest(&self, wal: &WalService) -> usize {
        let after = self.last_record_lsn();
        let batch = wal.stream_committed(after);
        let n = batch.len();
        if n == 0 {
            return 0;
        }
        let mut pages = self.pages.write();
        let mut last = after;
        for r in batch {
            let entry = pages.entry(r.page_key.clone()).or_default();
            entry.deltas.insert(r.lsn, r.delta);
            last = last.max(r.lsn);
        }
        *self.last_record_lsn.write() = last;
        n
    }

    /// commitLSN からの遅れが上限を超えていないか (バックプレッシャ)。
    pub fn check_replication_lag(&self, wal: &WalService) -> Result<u64, WalServiceError> {
        let lag = wal.commit_lsn().saturating_sub(self.last_record_lsn());
        if lag > self.max_replication_lag {
            return Err(WalServiceError::LagTooLarge { lag, limit: self.max_replication_lag });
        }
        Ok(lag)
    }

    /// 指定 LSN 時点のページを、image layer + delta layer から再構成する
    /// (`get_page_at_lsn`)。WAL が未着なら `WalNotArrived` を返す
    /// ——Neon の「対応する WAL が届くまで応答しない」保証に相当。
    pub fn get_page_at_lsn(&self, key: &str, lsn: Lsn) -> Result<Vec<u8>, WalServiceError> {
        let last = self.last_record_lsn();
        if lsn > last {
            return Err(WalServiceError::WalNotArrived { requested: lsn, last_record_lsn: last });
        }
        let pages = self.pages.read();
        let hist = pages.get(key).ok_or_else(|| WalServiceError::PageNotFound(key.to_string()))?;
        let mut page: Option<Vec<u8>> = match &hist.image {
            Some((image_lsn, bytes)) if *image_lsn <= lsn => Some(bytes.clone()),
            // image layer より前の LSN は delta が GC されており再構成できない
            Some((image_lsn, _)) => {
                return Err(WalServiceError::BelowGcCutoff { requested: lsn, image_lsn: *image_lsn })
            }
            None => None,
        };
        let from = hist.image.as_ref().map(|(l, _)| *l).filter(|l| *l <= lsn).unwrap_or(0);
        for (_, delta) in hist.deltas.range((std::ops::Bound::Excluded(from), std::ops::Bound::Included(lsn))) {
            match delta {
                PageDelta::Replace(b) => page = Some(b.clone()),
                PageDelta::Append(b) => {
                    let mut cur = page.take().unwrap_or_default();
                    cur.extend_from_slice(b);
                    page = Some(cur);
                }
            }
        }
        page.ok_or_else(|| WalServiceError::PageNotFound(key.to_string()))
    }

    /// 指定 LSN でページを materialize して image layer にする
    /// (Neon の compaction / image layer 生成相当)。それ以下の delta は
    /// 不要になるため破棄する——**過去 LSN の再構成能力はそこまで失われる**
    /// ため、GC 境界として扱う。
    pub fn create_image_layer(&self, key: &str, lsn: Lsn) -> Result<usize, WalServiceError> {
        let materialized = self.get_page_at_lsn(key, lsn)?;
        let mut pages = self.pages.write();
        let hist = pages.get_mut(key).ok_or_else(|| WalServiceError::PageNotFound(key.to_string()))?;
        let keep = hist.deltas.split_off(&(lsn.saturating_add(1)));
        let dropped = hist.deltas.len();
        hist.deltas = keep;
        hist.image = Some((lsn, materialized));
        Ok(dropped)
    }

    /// image layer が存在する LSN (GC 境界)。
    pub fn image_layer_lsn(&self, key: &str) -> Option<Lsn> {
        self.pages.read().get(key).and_then(|h| h.image.as_ref().map(|(l, _)| *l))
    }

    pub fn page_keys(&self) -> Vec<String> {
        self.pages.read().keys().cloned().collect()
    }
}

/// WAL サービス + pageserver を束ねた「ストレージ/コンピュート分離」構成。
/// 書き込み側 compute は `write` で WAL を投げるだけで、ページ再構成は
/// pageserver 側の責務になる。
#[derive(Debug)]
pub struct DisaggregatedStorage {
    pub wal: WalService,
    pub pageserver: Pageserver,
    term: RwLock<Term>,
}

impl DisaggregatedStorage {
    pub fn new(safekeepers: usize, max_replication_lag: u64) -> Self {
        let wal = WalService::with_safekeepers(safekeepers);
        let term = wal.start_proposer();
        Self { wal, pageserver: Pageserver::new(max_replication_lag), term: RwLock::new(term) }
    }

    pub fn term(&self) -> Term {
        *self.term.read()
    }

    /// この構成の proposer を再起動する (旧 proposer は fence される)。
    pub fn restart_proposer(&self) -> Term {
        let t = self.wal.start_proposer();
        *self.term.write() = t;
        t
    }

    /// 書き込み: WAL を quorum へ耐久化し、pageserver へ取り込ませる。
    /// 返り値は commitLSN。
    pub fn write(&self, records: &[WalRecord]) -> Result<Lsn, WalServiceError> {
        self.pageserver.check_replication_lag(&self.wal)?;
        let commit = self.wal.append(self.term(), records)?;
        self.pageserver.ingest(&self.wal);
        Ok(commit)
    }

    /// 読み取り: commitLSN 時点のページを取得する。
    pub fn read_latest(&self, key: &str) -> Result<Vec<u8>, WalServiceError> {
        self.pageserver.get_page_at_lsn(key, self.pageserver.last_record_lsn())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_lsn_is_the_quorum_th_largest_flush_lsn() {
        let wal = WalService::with_safekeepers(3);
        assert_eq!(wal.quorum(), 2);
        let term = wal.start_proposer();
        // 3台すべてへ届いた場合
        let commit = wal.append(term, &[WalRecord::replace(10, "p1", b"a".to_vec())]).unwrap();
        assert_eq!(commit, 10);
        // 1台だけ先へ進んでも commitLSN は進まない (quorum 番目=2番目に大きい値)
        wal.safekeeper(1).unwrap().accept(term, &[WalRecord::append(20, "p1", b"b".to_vec())]).unwrap();
        assert_eq!(wal.recompute_commit_lsn(), 10);
        // 2台目も届けば commitLSN が進む
        wal.safekeeper(2).unwrap().accept(term, &[WalRecord::append(20, "p1", b"b".to_vec())]).unwrap();
        assert_eq!(wal.recompute_commit_lsn(), 20);
    }

    #[test]
    fn commit_lsn_never_regresses() {
        let wal = WalService::with_safekeepers(3);
        let term = wal.start_proposer();
        wal.append(term, &[WalRecord::replace(5, "p", b"x".to_vec())]).unwrap();
        assert_eq!(wal.commit_lsn(), 5);
        // safekeeper が WAL を解放 (S3 へ落ちた分の破棄) しても commitLSN は後退しない
        for id in 1..=3 {
            wal.safekeeper(id).unwrap().truncate_up_to(5);
        }
        assert_eq!(wal.recompute_commit_lsn(), 5);
    }

    #[test]
    fn a_new_proposer_fences_the_old_one() {
        let wal = WalService::with_safekeepers(3);
        let old = wal.start_proposer();
        wal.append(old, &[WalRecord::replace(1, "p", b"v1".to_vec())]).unwrap();
        let new = wal.start_proposer();
        assert!(new > old);
        // 古い proposer の書き込みは quorum に届かず fence される (単一primary強制)
        let err = wal.append(old, &[WalRecord::replace(2, "p", b"stale".to_vec())]).unwrap_err();
        assert!(matches!(err, WalServiceError::Fenced { .. }), "got {err:?}");
        // 新しい proposer は書ける
        assert_eq!(wal.append(new, &[WalRecord::replace(2, "p", b"v2".to_vec())]).unwrap(), 2);
    }

    #[test]
    fn quorum_not_reached_when_majority_is_down() {
        let wal = WalService::with_safekeepers(3);
        let term = wal.start_proposer();
        // 2台を「先へ進めて」しまい、同じLSNの受理を失敗させる (NonMonotonicLsn)
        wal.safekeeper(1).unwrap().accept(term, &[WalRecord::replace(100, "p", b"a".to_vec())]).unwrap();
        wal.safekeeper(2).unwrap().accept(term, &[WalRecord::replace(100, "p", b"a".to_vec())]).unwrap();
        let err = wal.append(term, &[WalRecord::replace(50, "p", b"b".to_vec())]).unwrap_err();
        assert_eq!(err, WalServiceError::QuorumNotReached { acked: 1, needed: 2 });
    }

    #[test]
    fn safekeeper_rejects_non_monotonic_lsn() {
        let sk = Safekeeper::new(1);
        sk.accept(1, &[WalRecord::replace(10, "p", b"a".to_vec())]).unwrap();
        let err = sk.accept(1, &[WalRecord::append(10, "p", b"b".to_vec())]).unwrap_err();
        assert_eq!(err, WalServiceError::NonMonotonicLsn { lsn: 10, flush_lsn: 10 });
    }

    #[test]
    fn pageserver_reconstructs_page_at_a_historical_lsn() {
        let s = DisaggregatedStorage::new(3, DEFAULT_MAX_REPLICATION_LAG);
        s.write(&[WalRecord::replace(1, "page/a", b"base".to_vec())]).unwrap();
        s.write(&[WalRecord::append(2, "page/a", b"+d2".to_vec())]).unwrap();
        s.write(&[WalRecord::append(3, "page/a", b"+d3".to_vec())]).unwrap();
        assert_eq!(s.pageserver.get_page_at_lsn("page/a", 1).unwrap(), b"base".to_vec());
        assert_eq!(s.pageserver.get_page_at_lsn("page/a", 2).unwrap(), b"base+d2".to_vec());
        assert_eq!(s.read_latest("page/a").unwrap(), b"base+d2+d3".to_vec());
    }

    #[test]
    fn pageserver_refuses_reads_ahead_of_ingested_wal() {
        let s = DisaggregatedStorage::new(1, DEFAULT_MAX_REPLICATION_LAG);
        s.write(&[WalRecord::replace(1, "p", b"v".to_vec())]).unwrap();
        let err = s.pageserver.get_page_at_lsn("p", 99).unwrap_err();
        assert_eq!(err, WalServiceError::WalNotArrived { requested: 99, last_record_lsn: 1 });
    }

    #[test]
    fn image_layer_materializes_and_drops_older_deltas() {
        let s = DisaggregatedStorage::new(3, DEFAULT_MAX_REPLICATION_LAG);
        s.write(&[WalRecord::replace(1, "p", b"a".to_vec())]).unwrap();
        s.write(&[WalRecord::append(2, "p", b"b".to_vec())]).unwrap();
        s.write(&[WalRecord::append(3, "p", b"c".to_vec())]).unwrap();
        let dropped = s.pageserver.create_image_layer("p", 2).unwrap();
        assert_eq!(dropped, 2);
        assert_eq!(s.pageserver.image_layer_lsn("p"), Some(2));
        // image 以降は引き続き再構成できる
        assert_eq!(s.pageserver.get_page_at_lsn("p", 2).unwrap(), b"ab".to_vec());
        assert_eq!(s.pageserver.get_page_at_lsn("p", 3).unwrap(), b"abc".to_vec());
        // image 未満は delta が GC 済みで再構成できない (正直な限界を明示)
        assert_eq!(
            s.pageserver.get_page_at_lsn("p", 1).unwrap_err(),
            WalServiceError::BelowGcCutoff { requested: 1, image_lsn: 2 }
        );
    }

    #[test]
    fn backpressure_blocks_writes_when_pageserver_falls_behind() {
        let wal = WalService::with_safekeepers(1);
        let ps = Pageserver::new(5);
        let term = wal.start_proposer();
        wal.append(term, &[WalRecord::replace(100, "p", b"a".to_vec())]).unwrap();
        let err = ps.check_replication_lag(&wal).unwrap_err();
        assert_eq!(err, WalServiceError::LagTooLarge { lag: 100, limit: 5 });
        // 取り込めば解消する
        assert_eq!(ps.ingest(&wal), 1);
        assert_eq!(ps.check_replication_lag(&wal).unwrap(), 0);
    }

    #[test]
    fn safekeeper_wal_can_be_released_after_pageserver_ingests() {
        let s = DisaggregatedStorage::new(3, DEFAULT_MAX_REPLICATION_LAG);
        s.write(&[WalRecord::replace(1, "p", b"a".to_vec())]).unwrap();
        s.write(&[WalRecord::append(2, "p", b"b".to_vec())]).unwrap();
        let ingested_to = s.pageserver.last_record_lsn();
        assert_eq!(ingested_to, 2);
        let sk = s.wal.safekeeper(1).unwrap();
        assert_eq!(sk.truncate_up_to(ingested_to), 2);
        assert_eq!(sk.stream(0, 10).len(), 0);
        // pageserver 側は既に取り込んでいるので読み取りは成立し続ける
        assert_eq!(s.read_latest("p").unwrap(), b"ab".to_vec());
    }

    #[test]
    fn unknown_page_is_reported() {
        let s = DisaggregatedStorage::new(1, DEFAULT_MAX_REPLICATION_LAG);
        s.write(&[WalRecord::replace(1, "p", b"a".to_vec())]).unwrap();
        assert_eq!(
            s.pageserver.get_page_at_lsn("nope", 1).unwrap_err(),
            WalServiceError::PageNotFound("nope".to_string())
        );
        assert_eq!(s.pageserver.page_keys(), vec!["p".to_string()]);
    }
}
