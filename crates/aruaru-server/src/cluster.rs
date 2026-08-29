//! クラスタランタイム (Raft 書き込みパス統合)
//!
//! - [`EngineApplier`]: Raft が commit したコマンドを QueryEngine へ適用する状態機械
//! - [`build_cluster`]: NodeConfig から RaftNode + HttpTransport + RaftDriver を構築
//! - 書き込みは Leader が propose → 複製 → commit → EngineApplier で apply される
//!
//! ピア未指定 (単一ノード) のときは即 Leader 化し、propose 後にローカルで commit/apply する。

use std::collections::HashMap;
use std::sync::Arc;

use aruaru_dist::{BinaryTcpTransport, Command, CommandResponse, RaftDriver, RaftNode};
use aruaru_query::{QueryEngine, QueryResponse};

/// Raft commit を QueryEngine へ適用する状態機械
pub struct EngineApplier {
    engine: Arc<QueryEngine>,
}

impl EngineApplier {
    pub fn new(engine: Arc<QueryEngine>) -> Self {
        Self { engine }
    }
}

impl aruaru_dist::Applier for EngineApplier {
    fn apply(&self, command: &Command) -> CommandResponse {
        match command {
            Command::Exec(sql) => match self.engine.execute(sql) {
                Ok(_) => CommandResponse::ok(),
                Err(e) => CommandResponse::err(e),
            },
            Command::Commit(msg) => {
                let safe = msg.replace('\'', "''");
                match self.engine.execute(&format!("SELECT aruaru_commit('{safe}')")) {
                    Ok(_) => CommandResponse::ok(),
                    Err(e) => CommandResponse::err(e),
                }
            }
            Command::Noop => CommandResponse::ok(),
        }
    }
}

/// 同期マーカー (型エイリアス簡略化用)
pub type ClusterNode = RaftNode<EngineApplier>;
/// 【2026-08-29改修】ノード間RPCを`HttpTransport`(REST/JSON-over-HTTP)
/// から`BinaryTcpTransport`(生TCP上の長さプレフィックス付きバイナリ
/// フレーム、`aruaru_dist::raft::binary_transport`参照)へ切り替えた。
/// ユーザー指示「Raft/WALプロトコル系は一切REST APIを使用しないように」
/// への対応——理由・調査結果は`binary_transport.rs`モジュールdoc参照。
pub type ClusterDriver = RaftDriver<EngineApplier, BinaryTcpTransport>;

/// バイナリRaft/WALプロトコル用の待受ポートを、既存の`--gql-port`
/// (HTTP管理API)からの固定オフセットとして導出する
/// (`gql_port + BINARY_RAFT_PORT_OFFSET`)。新しいCLIフラグを増やさず、
/// 既存の`--peers`/`--learner-peers`(`id@host:gql_port`形式)を
/// そのまま流用できるようにするための約束事——両ノードとも同じ
/// オフセットを使う前提。
pub const BINARY_RAFT_PORT_OFFSET: u16 = 100;

/// `id@host:port`(`http://`スキーム有無どちらでも可)形式のピア文字列
/// から、バイナリRaftトランスポート用の`SocketAddr`(ポートは
/// `BINARY_RAFT_PORT_OFFSET`だけずらしたもの)を解決する。
fn to_binary_peer_addr(url_or_addr: &str) -> anyhow::Result<std::net::SocketAddr> {
    let stripped = url_or_addr
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let mut addr: std::net::SocketAddr = stripped
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid peer address '{url_or_addr}': {e}"))?;
    addr.set_port(addr.port() + BINARY_RAFT_PORT_OFFSET);
    Ok(addr)
}

/// クラスタを構築する。peers が空なら単一ノード(即Leader)。
/// 戻り値: (node, driver)。driver は呼び出し側で spawn する。
pub fn build_cluster(
    node_id: u64,
    peers: &[(u64, String)],
    engine: Arc<QueryEngine>,
) -> anyhow::Result<(Arc<ClusterNode>, Arc<ClusterDriver>)> {
    build_cluster_with_learners(node_id, peers, &[], false, engine)
}

/// 【2026-08-21新設・真のRaft learner化 第一歩】voter peer に加え、
/// learner peer (投票権を持たない非同期複製先)・自ノード自身が learner
/// として起動されるかどうかを指定できるクラスタ構築。
/// `aruaru-server --raft-role learner --peers <leaderのアドレス>`
/// のように、**同一マシン上の別プロセス・別ポート**として learner
/// ノードを実際に起動できるようにする(単一プロセス内購読からの前進)。
///
/// - `self_is_learner=false` (voter/leader 側): `learner_peers` は
///   Leader が AppendEntries を送るが quorum には数えない learner の
///   一覧 (peer_map には voter・learner 両方のアドレスを含める必要がある)。
/// - `self_is_learner=true` (learner 側プロセス): このノード自身は
///   投票せず、選挙にも参加しない。`peers` に Leader の (node_id, url)
///   を渡すことで、HttpTransport 経由で自分が受信した AppendEntries に
///   対する追加送信は行わないが(Learner は driver.rs 側で送信自体を
///   スキップする)、`node_id`解決のため peer_map は共有しておく。
pub fn build_cluster_with_learners(
    node_id: u64,
    peers: &[(u64, String)],
    learner_peers: &[(u64, String)],
    self_is_learner: bool,
    engine: Arc<QueryEngine>,
) -> anyhow::Result<(Arc<ClusterNode>, Arc<ClusterDriver>)> {
    let peer_ids: Vec<u64> = peers.iter().map(|(id, _)| *id).collect();
    let learner_ids: Vec<u64> = learner_peers.iter().map(|(id, _)| *id).collect();
    let applier = EngineApplier::new(engine);
    let node = Arc::new(RaftNode::new_with_learners(
        node_id,
        applier,
        peer_ids,
        learner_ids,
        self_is_learner,
    ));

    // 単一ノード (voter peer も learner peer も空) かつ自身が learner
    // でなければ即 Leader 化 (選挙不要)。learner 単独起動では Leader化しない
    // (learner は本質的に受動的なレプリカであり、自ら書き込みを提案しない)。
    if peers.is_empty() && learner_peers.is_empty() && !self_is_learner {
        node.become_leader();
        tracing::info!(node_id, "single-node cluster: promoted to leader");
    }

    let mut peer_map: HashMap<u64, String> = peers.iter().cloned().collect();
    peer_map.extend(learner_peers.iter().cloned());
    let mut binary_peer_map: HashMap<u64, std::net::SocketAddr> = HashMap::new();
    for (id, addr) in &peer_map {
        binary_peer_map.insert(*id, to_binary_peer_addr(addr)?);
    }
    let transport = Arc::new(BinaryTcpTransport::new(binary_peer_map));
    let driver = RaftDriver::new(node.clone(), transport);
    Ok((node, driver))
}

/// Leader として書き込み SQL を Raft 経由で提案・適用する。
/// 単一ノードでは即 commit/apply、複数ノードでは propose 後に driver が複製・commit する。
pub fn propose_write(node: &Arc<ClusterNode>, sql: &str) -> Result<u64, String> {
    let idx = node.propose(&Command::Exec(sql.to_string()))?;
    if node.peers().is_empty() {
        // 単一ノード: 即 commit + apply
        node.try_commit_to(idx);
        node.maybe_commit();
        node.apply_committed();
    }
    Ok(idx)
}

/// 書き込みコマンドのコミット (aruaru_commit) を Raft 経由で提案
pub fn propose_commit(node: &Arc<ClusterNode>, message: &str) -> Result<u64, String> {
    let idx = node.propose(&Command::Commit(message.to_string()))?;
    if node.peers().is_empty() {
        node.try_commit_to(idx);
        node.maybe_commit();
        node.apply_committed();
    }
    Ok(idx)
}

/// "id@host:port,id@host:port" 形式のピア指定をパース
pub fn parse_peers(spec: &str) -> Vec<(u64, String)> {
    spec.split(',')
        .filter_map(|s| {
            let s = s.trim();
            if s.is_empty() {
                return None;
            }
            let (id, addr) = s.split_once('@')?;
            let id: u64 = id.trim().parse().ok()?;
            let addr = addr.trim();
            // http スキーム補完
            let url = if addr.starts_with("http") {
                addr.to_string()
            } else {
                format!("http://{addr}")
            };
            Some((id, url))
        })
        .collect()
}

/// QueryResponse をテキスト1行に要約 (propose 応答用)
pub fn summarize(resp: QueryResponse) -> String {
    match resp {
        QueryResponse::Command { tag } => tag,
        QueryResponse::Rows { rows, .. } => format!("{} rows", rows.len()),
    }
}
