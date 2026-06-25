//! Raft トランスポート (ノード間 RPC の送信側)
//!
//! `Transport` を抽象化し、HTTP 実装 (`HttpTransport`) を提供する。
//! 受信側エンドポイント (`/raft/append`, `/raft/vote`) は aruaru-server が公開し、
//! 受け取った RPC を RaftNode のメソッドへ橋渡しする。

use std::collections::HashMap;

use async_trait::async_trait;

use super::rpc::{AppendEntriesReq, AppendEntriesResp, RequestVoteReq, RequestVoteResp};

/// ノード間 RPC の送信インタフェース
#[async_trait]
pub trait Transport: Send + Sync {
    async fn send_append_entries(
        &self,
        peer: u64,
        req: AppendEntriesReq,
    ) -> anyhow::Result<AppendEntriesResp>;

    async fn send_request_vote(
        &self,
        peer: u64,
        req: RequestVoteReq,
    ) -> anyhow::Result<RequestVoteResp>;
}

/// HTTP トランスポート (reqwest)。peer ノード ID → ベース URL を保持する。
pub struct HttpTransport {
    client: reqwest::Client,
    /// node_id → "http://host:port"
    peers: HashMap<u64, String>,
}

impl HttpTransport {
    pub fn new(peers: HashMap<u64, String>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;
        Ok(Self { client, peers })
    }

    fn base(&self, peer: u64) -> anyhow::Result<&str> {
        self.peers
            .get(&peer)
            .map(|s| s.as_str())
            .ok_or_else(|| anyhow::anyhow!("unknown peer: {peer}"))
    }
}

#[async_trait]
impl Transport for HttpTransport {
    async fn send_append_entries(
        &self,
        peer: u64,
        req: AppendEntriesReq,
    ) -> anyhow::Result<AppendEntriesResp> {
        let url = format!("{}/raft/append", self.base(peer)?);
        let resp = self
            .client
            .post(url)
            .json(&req)
            .send()
            .await?
            .json::<AppendEntriesResp>()
            .await?;
        Ok(resp)
    }

    async fn send_request_vote(
        &self,
        peer: u64,
        req: RequestVoteReq,
    ) -> anyhow::Result<RequestVoteResp> {
        let url = format!("{}/raft/vote", self.base(peer)?);
        let resp = self
            .client
            .post(url)
            .json(&req)
            .send()
            .await?
            .json::<RequestVoteResp>()
            .await?;
        Ok(resp)
    }
}
