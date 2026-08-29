//! バイナリRaft RPCトランスポート(REST/JSON-over-HTTPを使わない実装)
//!
//! ユーザー指示(2026-08-29、open-english経由で連携)「Raft/WALプロトコル
//! 系は一切REST APIを使用しないように。速度やセキュリティリスクなどを
//! 任せられる信頼できる代替が無ければ、Rust+RPoem/関連リポジトリを
//! フル動員して一から開発すること」への対応。
//!
//! ## 調査結果(日英Web検索、2026-08-29)
//! 実運用の分散合意システム(etcd・TiKV)はいずれもノード間RPCに
//! REST/JSON-over-HTTPを使わず、gRPC(Protocol Buffersによるバイナリ
//! シリアライズ+HTTP/2多重化)を使う。Protobufペイロードは同等の
//! JSONより概ね3〜5倍小さく、パースは5〜10倍速いという報告がある
//! (出典: TiKV公式ドキュメント`https://tikv.org/deep-dive/rpc/grpc/`、
//! etcd公式ドキュメント`https://etcd.io/docs/v3.4/learning/design-client/`)。
//! Raftのノード間通信はユーザー向けの汎用APIではなく、単一の運用者が
//! 管理する信頼済みネットワーク内で完結するRPCであるため、
//! 人間可読性・自己記述性(RESTの利点)よりも低レイテンシ・低
//! オーバーヘッドが優先される、という結論に達した。
//!
//! ## 採用した設計(このエコシステムの既存方針を踏襲)
//! tonic/gRPC等の外部フレームワークを新規導入するのではなく、この
//! エコシステム全体の一貫した方針(WunderGraph Cosmo/Poem/Tauriを
//! パッケージ依存させず概念だけ自前実装する、RPoemの手書きgRPC
//! Health Service`open-runo-router::grpc`が同種の前例)に倣い、
//! 生TCP上の**長さプレフィックス付きバイナリフレーム**を自前実装した。
//! シリアライズは`bincode`(既存のsha2/hex同様、狭い用途の薄いクレート
//! として許容——既存の`AppendEntriesReq`等の`serde::Serialize`実装を
//! そのまま再利用できるため、Protocol Buffers用に新しいスキーマ言語・
//! コード生成を持ち込む必要がない)。
//!
//! フレーム形式: `[4バイトBE長][bincodeエンコードされた本体]`。
//! 1リクエストにつき1TCP接続(接続の使い回しは行わない——HTTP/1.1の
//! 短命接続と同程度のシンプルさを意図的に維持し、複雑な接続プーリング・
//! 多重化は次段階の課題として残す)。
//!
//! ## 認証・正直な開示
//! 既存の`ARUARU_DB_ADMIN_TOKEN`をリクエスト本体に含め、受信側で
//! 定数時間比較する(`aruaru-server::admin::check_admin_auth`と同じ
//! 設計思想、タイミングサイドチャネル対策込み)。**TLS/mTLSによる
//! 暗号化は行っていない**——同一データセンター/信頼済みネットワーク
//! 内でのRaftトラフィックを前提とした、従来の`HttpTransport`
//! (平文HTTP+トークン認証)と同水準のセキュリティ境界を維持している
//! に留まる。これを超える保護(相互TLS証明書認証等)は次段階の課題。

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::closed_ts::{ClosedTimestampCoordinator, Timestamp};
use super::node::{Applier, RaftNode};
use super::rpc::{AppendEntriesReq, AppendEntriesResp, RequestVoteReq, RequestVoteResp};
use super::transport::Transport;

/// 1フレームの上限(16MiB)。暴走・DoS的な巨大フレームからの防御。
const MAX_FRAME_BYTES: u32 = 16 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
enum WireRequest {
    AppendEntries { token: Option<String>, req: AppendEntriesReq },
    RequestVote { token: Option<String>, req: RequestVoteReq },
    /// closed timestampのside transport(2026-08-24新設の`HttpSideTransport`
    /// が担っていたもの)。REST/JSON-over-HTTPを一切経由しない同一の
    /// バイナリ経路へ統合した(Raft/WALプロトコル系を1つの非RESTな
    /// TCPポートへまとめるため)。
    ClosedTsUpdate { token: Option<String>, updates: Vec<(u64, Timestamp)> },
}

#[derive(Debug, Serialize, Deserialize)]
enum WireResponse {
    AppendEntries(AppendEntriesResp),
    RequestVote(RequestVoteResp),
    ClosedTsUpdate { advanced: usize },
    Rejected(String),
}

/// タイミングサイドチャネル対策の定数時間比較。`aruaru-server::admin::
/// constant_time_eq`と全く同じロジックだが、クレート間で直接依存させず
/// (`aruaru-dist`は`aruaru-server`に依存しない設計のため)独立に複製した
/// ——このエコシステムの「小さなセキュリティユーティリティは概念だけ
/// 流用し直接依存はしない」という既存方針(Cosmo/Poem/Tauri同様)に倣う。
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    diff == 0
}

async fn write_frame<T: Serialize>(stream: &mut TcpStream, value: &T) -> anyhow::Result<()> {
    let bytes = bincode::serde::encode_to_vec(value, bincode::config::standard())?;
    if bytes.len() as u64 > MAX_FRAME_BYTES as u64 {
        anyhow::bail!("outgoing frame too large: {} bytes", bytes.len());
    }
    stream.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
    stream.write_all(&bytes).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_frame<T: for<'de> Deserialize<'de>>(stream: &mut TcpStream) -> anyhow::Result<T> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME_BYTES {
        anyhow::bail!("incoming frame too large: {len} bytes");
    }
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf).await?;
    let (value, _) = bincode::serde::decode_from_slice(&buf, bincode::config::standard())?;
    Ok(value)
}

/// バイナリTCPトランスポート(送信側)。既存の`Transport` traitを実装する
/// `HttpTransport`のドロップイン代替——`RaftDriver<A, BinaryTcpTransport>`
/// として同じ場所に差し込める。
pub struct BinaryTcpTransport {
    /// node_id → 待受アドレス(`host:port`)。
    peers: HashMap<u64, SocketAddr>,
    admin_token: Option<String>,
}

impl BinaryTcpTransport {
    pub fn new(peers: HashMap<u64, SocketAddr>) -> Self {
        let admin_token = std::env::var("ARUARU_DB_ADMIN_TOKEN").ok();
        Self { peers, admin_token }
    }

    fn addr(&self, peer: u64) -> anyhow::Result<SocketAddr> {
        self.peers
            .get(&peer)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("unknown peer: {peer}"))
    }
}

#[async_trait]
impl Transport for BinaryTcpTransport {
    async fn send_append_entries(
        &self,
        peer: u64,
        req: AppendEntriesReq,
    ) -> anyhow::Result<AppendEntriesResp> {
        let addr = self.addr(peer)?;
        let mut stream = TcpStream::connect(addr).await?;
        write_frame(
            &mut stream,
            &WireRequest::AppendEntries { token: self.admin_token.clone(), req },
        )
        .await?;
        match read_frame::<WireResponse>(&mut stream).await? {
            WireResponse::AppendEntries(resp) => Ok(resp),
            WireResponse::Rejected(reason) => anyhow::bail!("append_entries rejected: {reason}"),
            other => anyhow::bail!("unexpected response to AppendEntries: {other:?}"),
        }
    }

    async fn send_request_vote(
        &self,
        peer: u64,
        req: RequestVoteReq,
    ) -> anyhow::Result<RequestVoteResp> {
        let addr = self.addr(peer)?;
        let mut stream = TcpStream::connect(addr).await?;
        write_frame(
            &mut stream,
            &WireRequest::RequestVote { token: self.admin_token.clone(), req },
        )
        .await?;
        match read_frame::<WireResponse>(&mut stream).await? {
            WireResponse::RequestVote(resp) => Ok(resp),
            WireResponse::Rejected(reason) => anyhow::bail!("request_vote rejected: {reason}"),
            other => anyhow::bail!("unexpected response to RequestVote: {other:?}"),
        }
    }
}

/// closed timestampのside transport(送信側)。`HttpSideTransport`の
/// ドロップイン代替(REST/JSONではなく同じバイナリTCPフレームを使う)。
pub struct BinaryTcpSideTransport {
    peers: HashMap<u64, SocketAddr>,
    admin_token: Option<String>,
}

impl BinaryTcpSideTransport {
    pub fn new(peers: HashMap<u64, SocketAddr>) -> Self {
        let admin_token = std::env::var("ARUARU_DB_ADMIN_TOKEN").ok();
        Self { peers, admin_token }
    }

    fn addr(&self, peer: u64) -> anyhow::Result<SocketAddr> {
        self.peers
            .get(&peer)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("unknown peer: {peer}"))
    }

    /// 自ノードが保持するclosed timestampのスナップショットを、指定した
    /// follower peerへ実TCP経由で配布する。戻り値はfollower側で実際に
    /// 前進したRange数。
    pub async fn publish_to(&self, peer: u64, updates: Vec<(u64, Timestamp)>) -> anyhow::Result<usize> {
        let addr = self.addr(peer)?;
        let mut stream = TcpStream::connect(addr).await?;
        write_frame(
            &mut stream,
            &WireRequest::ClosedTsUpdate { token: self.admin_token.clone(), updates },
        )
        .await?;
        match read_frame::<WireResponse>(&mut stream).await? {
            WireResponse::ClosedTsUpdate { advanced } => Ok(advanced),
            WireResponse::Rejected(reason) => anyhow::bail!("closed-ts update rejected: {reason}"),
            other => anyhow::bail!("unexpected response to ClosedTsUpdate: {other:?}"),
        }
    }
}

/// バイナリTCPリスナー(受信側)。`bind_addr`で待ち受け、受信した
/// `WireRequest`を`node`(Raft AppendEntries/RequestVote)・
/// `closed_ts`(側チャネル、`None`なら`ClosedTsUpdate`は拒否)へ橋渡し
/// する——旧`/admin/raft/append`・`/admin/raft/vote`・`/admin/
/// closed-timestamp/receive`(いずれもREST)が担っていた役割を、
/// REST APIを一切経由せず**単一の非RESTなTCPポート**へ統合して
/// 引き継ぐ(ユーザー指示「Raft/WALプロトコル系は一切REST APIを
/// 使用しないように」への対応)。エラー時はループを止めず次の接続を
/// 待ち続ける。
pub async fn serve_binary_raft<A: Applier + 'static>(
    bind_addr: SocketAddr,
    node: Arc<RaftNode<A>>,
    closed_ts: Option<Arc<ClosedTimestampCoordinator>>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind_addr).await?;
    let expected_token = std::env::var("ARUARU_DB_ADMIN_TOKEN").ok();
    tracing::info!(addr = %bind_addr, "binary Raft/WAL RPC listener starting (REST不使用)");
    loop {
        let (mut stream, _peer_addr) = listener.accept().await?;
        let node = node.clone();
        let closed_ts = closed_ts.clone();
        let expected_token = expected_token.clone();
        tokio::spawn(async move {
            if let Err(e) =
                handle_connection(&mut stream, &node, closed_ts.as_deref(), expected_token.as_deref()).await
            {
                tracing::debug!(error = %e, "binary raft connection ended with error");
            }
        });
    }
}

async fn handle_connection<A: Applier>(
    stream: &mut TcpStream,
    node: &Arc<RaftNode<A>>,
    closed_ts: Option<&ClosedTimestampCoordinator>,
    expected_token: Option<&str>,
) -> anyhow::Result<()> {
    let req: WireRequest = read_frame(stream).await?;

    fn token_ok(provided: &Option<String>, expected: Option<&str>) -> bool {
        match expected {
            None => true, // 静的トークン未設定 = 従来のHttpTransportと同じく認証無し
            Some(expected) => {
                let provided = provided.as_deref().unwrap_or("");
                !provided.is_empty() && constant_time_eq(provided, expected)
            }
        }
    }

    let resp = match req {
        WireRequest::AppendEntries { token, req } => {
            if !token_ok(&token, expected_token) {
                write_frame(stream, &WireResponse::Rejected("invalid or missing token".into())).await?;
                return Ok(());
            }
            let result = node.append_entries(
                req.term,
                req.prev_log_index,
                req.prev_log_term,
                req.entries,
                req.leader_commit,
            );
            node.apply_committed();
            WireResponse::AppendEntries(AppendEntriesResp {
                term: result.term,
                success: result.success,
                match_index: result.match_index,
                from: node.node_id(),
            })
        }
        WireRequest::RequestVote { token, req } => {
            if !token_ok(&token, expected_token) {
                write_frame(stream, &WireResponse::Rejected("invalid or missing token".into())).await?;
                return Ok(());
            }
            let result = node.request_vote(
                req.term,
                req.candidate_id,
                req.last_log_index,
                req.last_log_term,
            );
            WireResponse::RequestVote(RequestVoteResp {
                term: result.term,
                vote_granted: result.granted,
                from: node.node_id(),
            })
        }
        WireRequest::ClosedTsUpdate { token, updates } => {
            if !token_ok(&token, expected_token) {
                write_frame(stream, &WireResponse::Rejected("invalid or missing token".into())).await?;
                return Ok(());
            }
            let advanced = match closed_ts {
                Some(coordinator) => coordinator.apply_closed_timestamp_updates(&updates),
                None => {
                    write_frame(
                        stream,
                        &WireResponse::Rejected("closed timestamp side transport is not enabled on this node".into()),
                    )
                    .await?;
                    return Ok(());
                }
            };
            WireResponse::ClosedTsUpdate { advanced }
        }
    };
    write_frame(stream, &resp).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raft::command::{Command, CommandResponse};
    use crate::raft::node::Applier as ApplierTrait;

    struct NoopApplier;
    impl ApplierTrait for NoopApplier {
        fn apply(&self, _command: &Command) -> CommandResponse {
            CommandResponse { ok: true, message: "noop".into() }
        }
    }

    #[tokio::test]
    async fn wire_frame_round_trips_append_entries() {
        // 実TCPループバック経由でフレームのエンコード/デコードを検証
        // (bincodeシリアライズが壊れていないことの直接証拠)。
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let req: WireRequest = read_frame(&mut stream).await.unwrap();
            match req {
                WireRequest::AppendEntries { token, req } => {
                    assert_eq!(token.as_deref(), Some("secret"));
                    assert_eq!(req.term, 7);
                    write_frame(
                        &mut stream,
                        &WireResponse::AppendEntries(AppendEntriesResp {
                            term: 7,
                            success: true,
                            match_index: 3,
                            from: 1,
                        }),
                    )
                    .await
                    .unwrap();
                }
                _ => panic!("expected AppendEntries"),
            }
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        write_frame(
            &mut client,
            &WireRequest::AppendEntries {
                token: Some("secret".into()),
                req: AppendEntriesReq {
                    term: 7,
                    leader_id: 1,
                    prev_log_index: 0,
                    prev_log_term: 0,
                    entries: vec![],
                    leader_commit: 0,
                },
            },
        )
        .await
        .unwrap();
        let resp: WireResponse = read_frame(&mut client).await.unwrap();
        match resp {
            WireResponse::AppendEntries(r) => {
                assert!(r.success);
                assert_eq!(r.match_index, 3);
            }
            _ => panic!("expected AppendEntries response"),
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn end_to_end_append_entries_over_real_tcp_via_transport_and_listener() {
        // BinaryTcpTransport(送信側) -> serve_binary_raft(受信側) ->
        // 実RaftNode、という一連の流れを実TCP経由で検証。REST/HTTPは
        // 一切経由しない。
        let node = Arc::new(RaftNode::new(1, NoopApplier, vec![2]));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener); // すぐ再bindするため一旦手放す(TOCTOUの余地は
                         // このテストの規模では無視できるほど小さい)。

        let server_node = node.clone();
        let server = tokio::spawn(async move {
            serve_binary_raft(addr, server_node, None).await.unwrap();
        });
        // リスナーが実際にacceptを開始するまで少し待つ。
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut peers = HashMap::new();
        peers.insert(1u64, addr);
        let transport = BinaryTcpTransport::new(peers);

        let resp = transport
            .send_append_entries(
                1,
                AppendEntriesReq {
                    term: 1,
                    leader_id: 1,
                    prev_log_index: 0,
                    prev_log_term: 0,
                    entries: vec![],
                    leader_commit: 0,
                },
            )
            .await
            .unwrap();
        // termが1で自ノードは元々term 0のFollower相当なので、素直な
        // ハートビート(空entries)は成功する。
        assert!(resp.success, "expected append_entries to succeed: {resp:?}");

        server.abort();
    }

    #[tokio::test]
    async fn rejects_when_token_mismatches() {
        std::env::set_var("ARUARU_DB_ADMIN_TOKEN", "correct-token");
        let node = Arc::new(RaftNode::new(1, NoopApplier, vec![]));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let server_node = node.clone();
        let server = tokio::spawn(async move {
            serve_binary_raft(addr, server_node, None).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut client = TcpStream::connect(addr).await.unwrap();
        write_frame(
            &mut client,
            &WireRequest::AppendEntries {
                token: Some("wrong-token".into()),
                req: AppendEntriesReq {
                    term: 1,
                    leader_id: 1,
                    prev_log_index: 0,
                    prev_log_term: 0,
                    entries: vec![],
                    leader_commit: 0,
                },
            },
        )
        .await
        .unwrap();
        let resp: WireResponse = read_frame(&mut client).await.unwrap();
        assert!(matches!(resp, WireResponse::Rejected(_)), "expected Rejected, got {resp:?}");

        server.abort();
        std::env::remove_var("ARUARU_DB_ADMIN_TOKEN");
    }
}
