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

    /// Raft ピアアドレス (カンマ区切り)
    #[arg(long)]
    peers: Option<String>,

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
    let admin_state = admin::AdminState::new(engine.clone(), registry.clone());
    // 【課金アイテムの権利消失防止】書き込みをRaft経由で複製するレプリケータ。
    // クラスタ構築に成功した場合のみ設定される (推奨構成: 自ノード+peers 2台=計3ノード)。
    let mut replicator: Option<std::sync::Arc<dyn aruaru_dist::ReplicatedWriter>> = None;
    match cluster::build_cluster(cli.raft_id, &peers, engine.clone()) {
        Ok((node, driver)) => {
            admin_state.attach_cluster(node.clone());
            if peers.is_empty() {
                tracing::info!(
                    node_id = cli.raft_id,
                    "Raft: single-node mode (leader). 本番運用では --peers で他2ノードを指定し、\
                     レプリケーション因子3(自ノード+2)にすることを推奨"
                );
            } else {
                tracing::info!(
                    node_id = cli.raft_id,
                    cluster_size = peers.len() + 1,
                    "Raft: multi-node cluster; consensus driver started (過半数コミットで書き込み確定)"
                );
            }
            replicator = Some(std::sync::Arc::new(aruaru_dist::RaftWriter::new(node)));
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
    let http_handle = tokio::spawn(async move {
        use poem::middleware::Cors;
        use poem::{get, handler, listener::TcpListener, EndpointExt, Route, Server};

        // Federation SDL を返すエンドポイント (wgc subgraph publish 用)
        #[handler]
        fn subgraph_sdl() -> String {
            aruaru_graphql::subgraph_sdl()
        }

        let app = Route::new()
            .at("/graphql", aruaru_graphql::graphql_endpoint(
                gql_engine.clone(),
                aruaru_graphql::AdminCtx {
                    engine: gql_engine.clone(),
                    registry: registry.clone(),
                    backup: backup_engine.clone(),
                },
            ))
            .at("/graphql/sdl", get(subgraph_sdl))
            .nest("/admin", admin::admin_routes(admin_state))
            // Web 版 Admin (別オリジン) からのアクセスを許可
            .with(Cors::new());
        tracing::info!(addr = %http_addr, "HTTP server (Cosmo subgraph /graphql + /admin) starting");
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
