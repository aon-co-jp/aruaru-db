//! クラスタランタイム (Raft 書き込みパス統合)
//!
//! - [`EngineApplier`]: Raft が commit したコマンドを QueryEngine へ適用する状態機械
//! - [`build_cluster`]: NodeConfig から RaftNode + HttpTransport + RaftDriver を構築
//! - 書き込みは Leader が propose → 複製 → commit → EngineApplier で apply される
//!
//! ピア未指定 (単一ノード) のときは即 Leader 化し、propose 後にローカルで commit/apply する。

use std::collections::HashMap;
use std::sync::Arc;

use aruaru_dist::{Command, CommandResponse, HttpTransport, RaftDriver, RaftNode};
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
pub type ClusterDriver = RaftDriver<EngineApplier, HttpTransport>;

/// クラスタを構築する。peers が空なら単一ノード(即Leader)。
/// 戻り値: (node, driver)。driver は呼び出し側で spawn する。
pub fn build_cluster(
    node_id: u64,
    peers: &[(u64, String)],
    engine: Arc<QueryEngine>,
) -> anyhow::Result<(Arc<ClusterNode>, Arc<ClusterDriver>)> {
    let peer_ids: Vec<u64> = peers.iter().map(|(id, _)| *id).collect();
    let applier = EngineApplier::new(engine);
    let node = Arc::new(RaftNode::new(node_id, applier, peer_ids));

    // 単一ノードは即 Leader 化 (選挙不要)
    if peers.is_empty() {
        node.become_leader();
        tracing::info!(node_id, "single-node cluster: promoted to leader");
    }

    let peer_map: HashMap<u64, String> = peers.iter().cloned().collect();
    let transport = Arc::new(HttpTransport::new(peer_map)?);
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
