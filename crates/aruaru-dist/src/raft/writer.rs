//! Raft経由の複製書き込みを型消去して公開するアダプタ
//!
//! `aruaru-wire` (pgwireサーバ) はクエリ実行時に、統合先の具体的な
//! `Applier` 実装を知る必要なく「書き込みをRaftに提案し、過半数コミット+適用
//! されるまで待つ」ことができるよう、この object-safe なトレイトを介して
//! `RaftNode<A>` を利用する。

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use super::command::Command;
use super::node::{Applier, RaftNode};

/// デフォルトの複製書き込み待ちタイムアウト (単一ノードでは即時完了)
pub const DEFAULT_COMMIT_TIMEOUT: Duration = Duration::from_secs(5);

#[async_trait]
pub trait ReplicatedWriter: Send + Sync {
    /// 書き込みSQLをRaft経由で提案し、過半数コミット+適用完了まで待つ。
    async fn write_sql(&self, sql: &str) -> Result<String, String>;

    /// バージョンコミット(aruaru_commit)をRaft経由で提案し、完了まで待つ。
    async fn write_commit(&self, message: &str) -> Result<String, String>;
}

/// `RaftNode<A>` を型消去して `ReplicatedWriter` として公開するラッパー
pub struct RaftWriter<A: Applier + 'static> {
    node: Arc<RaftNode<A>>,
    timeout: Duration,
}

impl<A: Applier + 'static> RaftWriter<A> {
    pub fn new(node: Arc<RaftNode<A>>) -> Self {
        Self { node, timeout: DEFAULT_COMMIT_TIMEOUT }
    }

    /// 単一ノード構成では propose 直後にローカルで commit+apply を進める。
    /// 複数ノードでは `RaftDriver::run` の背景ループが複製・多数決commitを進める。
    async fn propose_and_wait(&self, command: Command) -> Result<String, String> {
        let idx = self.node.propose(&command)?;
        if self.node.peers().is_empty() {
            self.node.try_commit_to(idx);
            self.node.maybe_commit();
            self.node.apply_committed();
        }
        let resp = self.node.wait_for_commit(idx, self.timeout).await?;
        if resp.ok {
            Ok(resp.message)
        } else {
            Err(resp.message)
        }
    }
}

#[async_trait]
impl<A: Applier + 'static> ReplicatedWriter for RaftWriter<A> {
    async fn write_sql(&self, sql: &str) -> Result<String, String> {
        self.propose_and_wait(Command::Exec(sql.to_string())).await
    }

    async fn write_commit(&self, message: &str) -> Result<String, String> {
        self.propose_and_wait(Command::Commit(message.to_string())).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use std::time::Duration;

    struct RecordingApplier {
        applied: Mutex<Vec<String>>,
    }
    impl Applier for RecordingApplier {
        fn apply(&self, command: &Command) -> super::super::command::CommandResponse {
            if let Command::Exec(sql) = command {
                self.applied.lock().push(sql.clone());
            }
            super::super::command::CommandResponse::ok()
        }
    }

    #[tokio::test]
    async fn test_single_node_write_completes_without_peers() {
        let node = Arc::new(RaftNode::new(1, RecordingApplier { applied: Mutex::new(vec![]) }, vec![]));
        node.become_leader();
        let writer = RaftWriter::new(node);
        let result = writer.write_sql("INSERT INTO items VALUES (1, 'sword')").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_write_times_out_if_quorum_never_reached() {
        // peers を持つが複製が来ない (ネットワーク層なし) ため、過半数コミットに到達しない
        let node = Arc::new(RaftNode::new(1, RecordingApplier { applied: Mutex::new(vec![]) }, vec![2, 3]));
        node.become_leader();
        let writer = RaftWriter { node, timeout: Duration::from_millis(100) };
        let result = writer.write_sql("INSERT INTO items VALUES (1, 'sword')").await;
        assert!(result.is_err(), "quorum未達なら書き込みは確定応答してはならない");
    }

    #[tokio::test]
    async fn test_write_completes_once_quorum_replication_is_simulated() {
        let node = Arc::new(RaftNode::new(1, RecordingApplier { applied: Mutex::new(vec![]) }, vec![2, 3]));
        node.become_leader();
        let writer = RaftWriter { node: node.clone(), timeout: Duration::from_secs(2) };

        // 別タスクで、あたかも2/3ノードから複製ACKが届いたかのように match_index を更新し、
        // maybe_commit + apply_committed を進める (実運用では RaftDriver::run が定期的に行う)
        let bg_node = node.clone();
        let bg = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            bg_node.update_match(2, 1); // peer 2 の複製ACKを模擬
            bg_node.maybe_commit();
            bg_node.apply_committed();
        });

        let result = writer.write_sql("INSERT INTO items VALUES (1, 'sword')").await;
        bg.await.unwrap();
        assert!(result.is_ok(), "2/3(過半数)の複製ACKが揃えばコミット確定するはず");
    }
}
