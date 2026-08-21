//! Raft トランスポート (ノード間 RPC の送信側)
//!
//! `Transport` を抽象化し、HTTP 実装 (`HttpTransport`) を提供する。
//! 受信側エンドポイント (`/admin/raft/append`, `/admin/raft/vote`) は
//! aruaru-server が `/admin/*` 配下 (`admin::admin_routes`) に公開し、
//! 受け取った RPC を RaftNode のメソッドへ橋渡しする。`/admin/*` 全体が
//! `x-admin-token` 認証(`ARUARU_DB_ADMIN_TOKEN`)配下にあるため、
//! この送信側もクラスタの全ノードで同じトークンを共有する前提で
//! ヘッダーを付与する。

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
    /// 【2026-08-21実バグ修正】`/raft/append`・`/raft/vote`は
    /// 2026-07-30に`/admin/*`全体へ`x-admin-token`認証(環境変数
    /// `ARUARU_DB_ADMIN_TOKEN`)が遡及適用されたが、当時のHANDOFFは
    /// 「これらを実際に呼ぶノード間通信はまだ配線されていないため
    /// 実害は無い」と正直に記していた通り、この`HttpTransport`
    /// (実際にノード間通信を行う唯一の経路)側にはヘッダー送信が
    /// 一度も実装されていなかった。今回、実際に複数プロセスの
    /// Raftクラスタ(voter+learner)を実起動して検証したところ、
    /// 認証ミドルウェアがAppendEntries/RequestVoteを401で全て
    /// 拒否し、ログの複製が一切成立しない実バグとして顕在化した
    /// (`reqwest`の`.json::<T>()`がエラーレスポンスのボディを
    /// パースできず`Err`となり、`driver.rs`側では`tracing::debug!`
    /// でしか記録されないため、通常運用では気づきにくい形で
    /// サイレントに複製が止まっていた)。起動時の環境変数を1回だけ
    /// 読み取り、送信ノード側も受信ノード側(`admin.rs::check_admin_
    /// auth`)と同じ`ARUARU_DB_ADMIN_TOKEN`を共有する前提で全リクエスト
    /// に付与する(未設定ならヘッダーを送らず、受信側が503を返す
    /// ——認証を要求しない構成同士でも従来通り動作する)。
    admin_token: Option<String>,
}

impl HttpTransport {
    pub fn new(peers: HashMap<u64, String>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;
        let admin_token = std::env::var("ARUARU_DB_ADMIN_TOKEN").ok();
        Ok(Self { client, peers, admin_token })
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
        // 【2026-08-21実バグ修正その2】`aruaru-server/src/main.rs`は
        // `admin::admin_routes(..)`全体を`.nest("/admin", ..)`で
        // マウントしているため、受信側の実パスは`/admin/raft/append`
        // (`/raft/append`ではない)。旧コードはネスト前の古いパス
        // 前提のままで、実際に複数プロセスを起動して検証するまで
        // 気づかれていなかった(404が`tracing::debug!`止まりで
        // 通常運用では見えないため)。
        let url = format!("{}/admin/raft/append", self.base(peer)?);
        let mut builder = self.client.post(url).json(&req);
        if let Some(token) = &self.admin_token {
            builder = builder.header("x-admin-token", token);
        }
        let resp = builder.send().await?.error_for_status()?.json::<AppendEntriesResp>().await?;
        Ok(resp)
    }

    async fn send_request_vote(
        &self,
        peer: u64,
        req: RequestVoteReq,
    ) -> anyhow::Result<RequestVoteResp> {
        let url = format!("{}/admin/raft/vote", self.base(peer)?);
        let mut builder = self.client.post(url).json(&req);
        if let Some(token) = &self.admin_token {
            builder = builder.header("x-admin-token", token);
        }
        let resp = builder.send().await?.error_for_status()?.json::<RequestVoteResp>().await?;
        Ok(resp)
    }
}
