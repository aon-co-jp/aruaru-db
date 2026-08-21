//! aruaru-server: メインエントリポイント
//!
//! 起動フロー:
//! 1. 設定ロード (TOML / 環境変数 / CLI フラグ)
//! 2. Storage Engine 起動
//! 3. VersionController 初期化
//! 4. HTAP Query Engine 起動
//! 5. pgwire サーバ起動 (:5432)
//! 6. GraphQL/REST サーバ起動 (:4000)
//! 7. (クラスタモード) openraft ノード起動

use clap::Parser;
use tracing_subscriber::EnvFilter;

mod admin;
mod cluster;
mod ephemeral_pod;
mod self_update;

/// aruaru-DB server
#[derive(Debug, Parser)]
#[command(name = "aruaru-server", version, about)]
struct Cli {
    /// データディレクトリ
    #[arg(long, default_value = "./data")]
    data: String,

    /// PostgreSQL ワイヤポート
    #[arg(long, default_value = "5432")]
    pg_port: u16,

    /// GraphQL HTTP ポート
    #[arg(long, default_value = "4000")]
    gql_port: u16,

    /// Raft ノード ID (シングルノードは 1)
    #[arg(long, default_value = "1")]
    raft_id: u64,

    /// Raft ピアアドレス (カンマ区切り、投票権を持つ voter peer)
    #[arg(long)]
    peers: Option<String>,

    /// 【2026-08-21新設】このノード自身の Raft ロール。"voter"(既定、
    /// leader/follower として振る舞う)または "learner"(投票権を持たず
    /// Leader からの複製をひたすら受け取るだけの非同期レプリカ)。
    /// 同一マシン上で `--raft-role voter --port 5433` と
    /// `--raft-role learner --port 5434` のように**別プロセス**として
    /// 起動し、`--peers`(learner側は接続先leaderのURL)/
    /// `--learner-peers`(leader側が複製先として認識するlearnerのURL)を
    /// 指定することで、実際にTCP越しにログが複製される真のマルチ
    /// プロセスRaftを構成できる。
    #[arg(long, default_value = "voter")]
    raft_role: String,

    /// 【2026-08-21新設】leader/voter側から見た learner peer アドレス
    /// (カンマ区切り、`--peers` と同じ "id@host:port" 形式)。
    /// Leader はここへ AppendEntries を複製するが、quorum(過半数コミット
    /// の判定)には数えない。
    #[arg(long)]
    learner_peers: Option<String>,

    /// 【2026-08-21新設・ephemeral SQL pod化 第一歩】このプロセスを
    /// 「使い捨て計算ワーカー」として起動する内部フラグ。標準入力から
    /// JSON({tenant_id, tables: [(name,cols,rows)], sql})を1件読み取り、
    /// 完全に独立したインメモリQueryEngineでテーブルを再構築してSQLを
    /// 1回だけ実行し、結果をJSONで標準出力へ書いて即座に終了する
    /// (永続ストレージ・pgwire・GraphQL・Raftは一切起動しない)。
    /// `ephemeral_pod::run_ephemeral_query` が
    /// `tokio::process::Command`でこのフラグ付きの自分自身を子プロセスと
    /// して起動し、処理完了後にそのプロセスは終了する
    /// (CockroachDB Serverless の ephemeral SQL pod の発想を、単一マシン
    /// 上のプロセスレベル分離で模した最小実装)。
    #[arg(long, default_value_t = false)]
    ephemeral_worker: bool,

    /// ログレベル (trace/debug/info/warn/error)
    #[arg(long, default_value = "info")]
    log_level: String,

    /// 【第1層】TLS証明書 (PEM)。未指定時は平文TCP (開発用)
    #[arg(long)]
    tls_cert: Option<String>,

    /// 【第1層】TLS秘密鍵 (PEM, PKCS8)
    #[arg(long)]
    tls_key: Option<String>,

    /// 【第2層】mTLS: クライアント証明書検証用CA証明書 (指定時はクライアント証明書必須)
    #[arg(long)]
    require_client_cert: Option<String>,

    /// 【UDP経路】QUICリスナーのポート (未指定時はQUIC無効)
    #[arg(long)]
    quic_port: Option<u16>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // 【ephemeral SQL pod化】自分自身が使い捨てワーカーとして起動された
    // 場合は、通常のサーバ起動フロー(永続ストレージ・pgwire・GraphQL・
    // Raft)を一切経由せず、標準入力から1件だけリクエストを処理して即終了する。
    if cli.ephemeral_worker {
        return ephemeral_pod::run_worker_once();
    }

    // ── ロギング初期化 ─────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(&cli.log_level)),
        )
        .json()
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        data    = %cli.data,
        pg_port = cli.pg_port,
        gql_port = cli.gql_port,
        raft_id  = cli.raft_id,
        "aruaru-DB starting 🦀"
    );

    // ── 共有クエリエンジン (ストレージ + Git-on-SQL) ──────────
    let engine = std::sync::Arc::new(aruaru_query::QueryEngine::new());

    // ── 永続ストレージ (fjall) を開いて復元し、エンジンへ取り付け ──
    match aruaru_core::PersistentStore::open(&cli.data) {
        Ok(store) => {
            let store = std::sync::Arc::new(store);
            match engine.load_from(&store) {
                Ok(n) => tracing::info!(tables = n, path = %cli.data, "restored tables from fjall store"),
                Err(e) => tracing::warn!(error = %e, "failed to restore from store"),
            }
            // 以後 aruaru_commit ごとに自動 persist
            engine.attach_store(store);
            tracing::info!("auto-persist on commit enabled");
        }
        Err(e) => tracing::warn!(error = %e, path = %cli.data, "could not open persistent store (in-memory only)"),
    }

    // ── DUAL DATABASE構成: aruaru-db × 実PostgreSQL(2026-07-20追記) ──
    // `DUAL_DATABASE_URL` 環境変数(未設定時はミラー無効、既存動作を
    // 変えない)。commit_hookはfire-and-forget(`tokio::spawn`)であり、
    // `aruaru_query::engine::QueryEngine::set_commit_hook`のdocに記載の
    // 通り、真の同期ポリシーからの意図的な逸脱である(engineのasync化を
    // 要する将来課題)。ミラー失敗はcommit自体の成功/失敗に影響しない。
    if let Ok(dual_db_url) = std::env::var("DUAL_DATABASE_URL") {
        match sqlx::PgPool::connect(&dual_db_url).await {
            Ok(pool) => {
                let mirror = std::sync::Arc::new(aruaru_dist::DualDatabaseMirror::new(pool));
                match mirror.ensure_schema().await {
                    Ok(()) => {
                        tracing::info!("DUAL DATABASE: PostgreSQL mirror schema ready");
                        let mirror_for_hook = mirror.clone();
                        engine.set_commit_hook(std::sync::Arc::new(move |commit_id: &str, rows: &[(String, String, String)]| {
                            let mirror = mirror_for_hook.clone();
                            let commit_id = commit_id.to_string();
                            let rows = rows.to_vec();
                            tokio::spawn(async move {
                                for (table_name, row_key, payload_json) in rows {
                                    let mutation = aruaru_dist::MirroredMutation {
                                        table_name,
                                        row_key,
                                        payload_json,
                                        commit_id: commit_id.clone(),
                                        committed_at: chrono::Utc::now(),
                                    };
                                    if let Err(e) = mirror.mirror(&mutation).await {
                                        tracing::error!(error = %e, commit = %commit_id, "DUAL DATABASE mirror failed for this commit's row (aruaru-db commit itself is unaffected)");
                                    }
                                }
                            });
                        }));
                        tracing::info!("DUAL DATABASE: commit hook registered (aruaru-db -> PostgreSQL mirror)");
                    }
                    Err(e) => tracing::error!(error = %e, "DUAL DATABASE: ensure_schema failed; mirror disabled"),
                }
            }
            Err(e) => tracing::error!(error = %e, url = %dual_db_url, "DUAL DATABASE: failed to connect to PostgreSQL; mirror disabled"),
        }
    } else {
        tracing::debug!("DUAL_DATABASE_URL not set; DUAL DATABASE mirror disabled");
    }

    // ── バックアップエンジン (ローカル: <data>/backups) ────────
    let backup_config = aruaru_backup::BackupConfig {
        destination: aruaru_backup::BackupDestination::Local {
            path: std::path::PathBuf::from(&cli.data).join("backups"),
        },
        kind: aruaru_backup::BackupKind::Full,
        compression: aruaru_backup::BackupCompression::None,
        encrypt: false,
        retention_days: 30,
    };
    let backup_engine = std::sync::Arc::new(aruaru_backup::BackupEngine::new(
        backup_config,
        engine.clone(),
    ));

    // ── 対応DBレジストリ (150+件) + 毎日クロール ──────────────
    let registry = aruaru_registry::Registry::new();
    tracing::info!(databases = registry.len(), "loaded supported-database registry");
    let crawl_registry = registry.clone();
    let _crawl_handle = tokio::spawn(async move {
        aruaru_registry::scheduler::run_daily(crawl_registry).await;
    });

    // ── Raft クラスタ構築 ─────────────────────────────────────
    let peers = cli
        .peers
        .as_deref()
        .map(cluster::parse_peers)
        .unwrap_or_default();
    let learner_peers = cli
        .learner_peers
        .as_deref()
        .map(cluster::parse_peers)
        .unwrap_or_default();
    let self_is_learner = match cli.raft_role.as_str() {
        "learner" => true,
        "voter" => false,
        other => {
            tracing::warn!(raft_role = other, "unknown --raft-role value; defaulting to voter");
            false
        }
    };
    let admin_state = admin::AdminState::new(engine.clone(), registry.clone());
    // 【2026-08-21新設・Vitess Reshard/VTGate scatter-gatherの実配線】
    // 既存の単一`ClusterNode`(本番のOLTP書き込み経路、pgwire/GraphQL/REST
    // `/admin/cluster/propose`が実際に使う)とは独立した、
    // `MultiRaftCluster`(Range単位の独立Raftグループ)を単一ノード構成で
    // 初期化しAdminStateへ取り付ける。既存の書き込み経路には一切触れない
    // オプトイン方式(`/admin/multi-raft/*`からのみ操作可能)。
    let multi_raft_cluster = std::sync::Arc::new(aruaru_dist::MultiRaftCluster::single_node(
        cli.raft_id,
        format!("127.0.0.1:{}", cli.gql_port),
        cluster::EngineApplier::new(engine.clone()),
    ));
    admin_state.attach_multi_raft(multi_raft_cluster);
    // 【課金アイテムの権利消失防止】書き込みをRaft経由で複製するレプリケータ。
    // クラスタ構築に成功した場合のみ設定される (推奨構成: 自ノード+peers 2台=計3ノード)。
    let mut replicator: Option<std::sync::Arc<dyn aruaru_dist::ReplicatedWriter>> = None;
    match cluster::build_cluster_with_learners(
        cli.raft_id,
        &peers,
        &learner_peers,
        self_is_learner,
        engine.clone(),
    ) {
        Ok((node, driver)) => {
            admin_state.attach_cluster(node.clone());
            if self_is_learner {
                tracing::info!(
                    node_id = cli.raft_id,
                    "Raft: learner mode (別プロセスの非同期レプリカ、投票権なし)。\
                     Leader 側は --learner-peers にこのノードのアドレスを含める必要がある"
                );
            } else if peers.is_empty() && learner_peers.is_empty() {
                tracing::info!(
                    node_id = cli.raft_id,
                    "Raft: single-node mode (leader). 本番運用では --peers で他2ノードを指定し、\
                     レプリケーション因子3(自ノード+2)にすることを推奨"
                );
            } else {
                tracing::info!(
                    node_id = cli.raft_id,
                    cluster_size = peers.len() + 1,
                    learners = learner_peers.len(),
                    "Raft: multi-node cluster; consensus driver started (過半数コミットで書き込み確定)"
                );
            }
            let raft_writer: std::sync::Arc<dyn aruaru_dist::ReplicatedWriter> =
                std::sync::Arc::new(aruaru_dist::RaftWriter::new(node));
            // gap (b)/(c) 対応(2026-07-25追記): pgwireサーバへ渡すのと同一の
            // Arc<dyn ReplicatedWriter> を AdminState にも取り付ける。これにより
            // (b) 管理API(POST /admin/disaster-email-backup)が稼働中のこの
            // インスタンスへ set_disaster_email_backup で実際に注入でき、
            // (c) 管理API経由の書き込み(/admin/cluster/propose)も RaftNode
            // 直接経路ではなくこの RaftWriter を経由するようになる。
            admin_state.attach_replicator(raft_writer.clone());
            replicator = Some(raft_writer);
            // 合意ランタイムを常駐
            let _raft_handle = tokio::spawn(async move {
                driver.run().await;
            });
        }
        Err(e) => tracing::warn!(error = %e, "failed to build Raft cluster; running without consensus"),
    }

    // ── HTTP サーバ (Poem): GraphQL(Cosmoサブグラフ) + 管理REST を同居 ──
    let http_addr = format!("0.0.0.0:{}", cli.gql_port);
    let gql_engine = engine.clone();
    let gql_replicator = replicator.clone();
    let http_handle = tokio::spawn(async move {
        use poem::middleware::Cors;
        use poem::{get, handler, listener::TcpListener, EndpointExt, Route, Server};

        // Federation SDL を返すエンドポイント (wgc subgraph publish 用)
        #[handler]
        fn subgraph_sdl() -> String {
            aruaru_graphql::subgraph_sdl()
        }

        // ヘルスチェック(2026-08-19新設、self_update.rs参照)。
        #[handler]
        fn healthz() -> &'static str {
            "ok"
        }

        let app = Route::new()
            .at("/healthz", get(healthz))
            .at("/graphql", aruaru_graphql::graphql_endpoint(
                gql_engine.clone(),
                aruaru_graphql::AdminCtx {
                    engine: gql_engine.clone(),
                    registry: registry.clone(),
                    backup: backup_engine.clone(),
                    // 2026-07-26追記: pgwireサーバ(wire_config.replicator)・
                    // REST admin API(admin_state.attach_replicator)と同一の
                    // Arc<dyn ReplicatedWriter> をGraphQL側にも共有する
                    // (cluster_propose resolverのRaftWriter経由化のため)。
                    replicator: gql_replicator.clone(),
                },
            ))
            .at("/graphql/sdl", get(subgraph_sdl))
            .nest("/admin", admin::admin_routes(admin_state))
            // Web 版 Admin (別オリジン) からのアクセスを許可
            .with(Cors::new());
        tracing::info!(addr = %http_addr, "HTTP server (Cosmo subgraph /graphql + /admin) starting");
        // 自動アップデート機能(2026-08-19新設、既定off、self_update.rs参照)。
        if let Ok(addr) = http_addr.parse::<std::net::SocketAddr>() {
            tokio::spawn(self_update::check_and_apply_update(addr));
        }
        if let Err(e) = Server::new(TcpListener::bind(&http_addr)).run(app).await {
            tracing::error!("HTTP server error: {e}");
        }
    });

    // ── pgwire サーバ ───────────────────────────────────
    let pg_addr = format!("0.0.0.0:{}", cli.pg_port);
    let tls_config = match (&cli.tls_cert, &cli.tls_key) {
        (Some(cert_path), Some(key_path)) => Some(aruaru_wire::tls::TlsConfig {
            cert_path: cert_path.clone(),
            key_path: key_path.clone(),
            client_ca_path: cli.require_client_cert.clone(),
        }),
        _ => None,
    };
    let wire_config = aruaru_wire::WireServerConfig {
        bind_addr: pg_addr,
        database_name: "aruaru".to_string(),
        tls: tls_config,
        replicator,
    };
    let wire_engine = engine.clone();
    let wire_handle = tokio::spawn(async move {
        if let Err(e) = aruaru_wire::start_wire_server(wire_config, wire_engine).await {
            tracing::error!("Wire server error: {e}");
        }
    });

    // ── シャットダウン待機 ──────────────────────────────
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Shutdown signal received");
        }
        _ = http_handle => {}
        _ = wire_handle => {}
    }

    tracing::info!("aruaru-DB stopped");
    Ok(())
}
