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
mod columnar_pod;
mod config;
mod ephemeral_pod;
mod self_update;

/// aruaru-DB server
#[derive(Debug, Parser)]
#[command(name = "aruaru-server", version, about)]
struct Cli {
    /// 宣言的設定ファイル `aruaru.yaml` のパス(任意)。指定すると
    /// 起動時に読み込み、`watch_config.enabled` ならホットリロード監視も
    /// 開始する。未指定なら従来どおり CLI フラグのみで動作する。
    /// 設計: docs/CONTROL_PLANE_REDESIGN.md
    #[arg(long)]
    config: Option<String>,

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

    /// 【2026-08-31新設・A.6-2実プロセス検証用】このプロセスを
    /// `ColumnarApplier`(行→列非同期変換レプリカ)を注入した learner
    /// として起動する。`--raft-role learner --peers <leaderのアドレス>`
    /// と組み合わせて使う——通常のlearner(`EngineApplier`、行ストアの
    /// 単純複製)とは別の、TiFlash型の列レプリカ検証専用フロー。
    /// 有効時は通常のpgwire/GraphQLサーバーを一切起動せず、Raft driverと
    /// 最小限の観測用HTTP(`/healthz`・`GET /columnar/:table`)のみを
    /// 提供して待ち受け続ける(`--gql-port`をその待受ポートとして流用)。
    #[arg(long, default_value_t = false)]
    columnar_learner: bool,

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

    // 【A.6-2実プロセス検証用・ColumnarApplier learner】通常のpgwire/
    // GraphQLサーバー起動フローを一切経由せず、`ColumnarApplier`を
    // 注入したRaft learnerとして待ち受け続ける。`--raft-role learner
    // --peers <leaderのアドレス> --columnar-learner`で起動する。
    if cli.columnar_learner {
        if cli.raft_role != "learner" {
            tracing::warn!(
                raft_role = %cli.raft_role,
                "--columnar-learner は --raft-role learner と組み合わせて使う想定です(続行します)"
            );
        }
        let leader_peers = cli
            .peers
            .as_deref()
            .map(cluster::parse_peers)
            .unwrap_or_default();
        let applier = std::sync::Arc::new(aruaru_dist::ColumnarApplier::with_in_memory_store(
            std::sync::Arc::new(aruaru_query::QueryEngine::new()),
        ));
        let (node, driver) =
            cluster::build_columnar_learner_cluster(cli.raft_id, &leader_peers, applier.clone())?;
        let binary_bind_addr: std::net::SocketAddr =
            format!("0.0.0.0:{}", cli.gql_port + cluster::BINARY_RAFT_PORT_OFFSET).parse()?;
        let _raft_handle = tokio::spawn(async move {
            driver.run().await;
        });
        let _binary_handle = tokio::spawn(async move {
            if let Err(e) = aruaru_dist::serve_binary_raft(binary_bind_addr, node, None).await {
                tracing::error!(error = %e, "binary raft listener (columnar learner) exited");
            }
        });
        columnar_pod::serve(format!("0.0.0.0:{}", cli.gql_port), applier).await?;
        return Ok(());
    }

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

    // ── 宣言的設定 `aruaru.yaml`(任意) ─────────────────────
    // 設計の正本: docs/CONTROL_PLANE_REDESIGN.md(P1)。`--config` 指定時のみ
    // 読み込み・初回 reconcile・ホットリロード監視を行う。未指定なら
    // 従来どおり CLI フラグのみで動作(後方互換)。
    if let Some(config_path) = cli.config.clone() {
        match config::AruaruConfig::load(&config_path) {
            Ok(cfg) => {
                let report = config::reconcile(&cfg, None, &admin_state);
                tracing::info!(path = %config_path, ?report, "aruaru.yaml を読み込み、初回 reconcile を適用しました");
                config::spawn_config_watcher(
                    std::path::PathBuf::from(&config_path),
                    cfg,
                    admin_state.clone(),
                );
            }
            Err(e) => {
                tracing::error!(error = %e, path = %config_path, "aruaru.yaml の読み込みに失敗。CLI フラグのみで起動を続行します");
            }
        }
    }
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
    // GraphQL側(`AdminCtx.multi_raft`)へ同一インスタンスを共有するため、
    // `attach_multi_raft`で消費される前に複製しておく(`keyring_for_
    // graphql`等と同じパターン、2026-08-31 trait注入リファクタ)。
    let multi_raft_for_graphql = multi_raft_cluster.clone();
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
            // 【2026-08-29新設】バイナリRaft/WALプロトコルリスナー
            // (`aruaru_dist::serve_binary_raft`、REST/JSON-over-HTTPを
            // 一切使わない生TCPバイナリ実装、詳細は`binary_transport.rs`
            // モジュールdoc参照)を、既存のHTTP管理APIポートから固定
            // オフセット(`cluster::BINARY_RAFT_PORT_OFFSET`)ずらした
            // ポートで起動する。ここがAppendEntries/RequestVote・closed
            // timestamp side transportの実際の受信口になる——
            // `/admin/raft/*`・`/admin/closed-timestamp/receive`
            // (REST)はもはやノード間通信の主経路ではない。
            let binary_bind_addr: std::net::SocketAddr = format!(
                "0.0.0.0:{}",
                cli.gql_port + cluster::BINARY_RAFT_PORT_OFFSET
            )
            .parse()
            .expect("valid binary raft bind address");
            let binary_listener_node = node.clone();
            let binary_listener_closed_ts = admin_state.closed_ts_coordinator();
            tokio::spawn(async move {
                if let Err(e) = aruaru_dist::serve_binary_raft(
                    binary_bind_addr,
                    binary_listener_node,
                    Some(binary_listener_closed_ts),
                )
                .await
                {
                    tracing::error!(error = %e, "binary Raft/WAL listener exited");
                }
            });
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

        // GraphQL `keyStatus` query / `revokeKeys` mutation が参照する
        // KeyGuardian共有ハンドル(2026-08-29(続き)新設。続き4でREST
        // `/admin/keys/*`は撤廃し、GraphQLが唯一の管理経路になった)。
        let keyring_for_graphql = admin_state.keyring.clone();
        // GraphQL `clusterStatus`がREST `/admin/cluster`と同じトポロジを
        // 参照するための共有ハンドル(2026-08-29新設)。
        let topology_for_graphql = admin_state.topology_handle();
        // バックアップスケジュール・フェデレーションソースも同様に共有
        // (2026-08-29新設、`backup_schedule`/`federated_sources`の
        // GraphQLスタブをREST実状態へ接続する対応)。
        let schedule_for_graphql = admin_state.schedule_handle();
        let federation_for_graphql = admin_state.federation_handle();
        // オブジェクトテーブル(時間旅行=VersionlessAPIの実体)も同一
        // インスタンスをGraphQL `objectTable`へ共有(2026-08-29(続き3))。
        let object_table_for_graphql = admin_state.object_table_handle();
        // 【2026-08-29 再設計 P2】並列設定(4フィールド共有型)。GraphQL
        // `parallelConfig` query が `aruaru.yaml: query.parallel` の実効値を
        // 返すために同一インスタンスを共有する。
        let parallel_for_graphql = admin_state.parallel_handle();
        // 【2026-08-29 再設計 P3(続き10)】closed timestamp / WAL サービス /
        // shard-per-core ストアも REST(`AdminState`)と同一インスタンスを
        // GraphQL(`closedTimestamp`/`walService`/`shardedStore*` 系)へ共有。
        // 旧 REST `/admin/closed-timestamp/{status,range,advance,plan}`・
        // `/admin/wal-service/*`・`/admin/sharded-store*` は撤廃済み。
        let closed_ts_for_graphql = admin_state.closed_ts_coordinator();
        let wal_storage_for_graphql = admin_state.wal_storage_handle();
        let sharded_store_for_graphql = admin_state.sharded_store_handle();
        // 【2026-08-31 trait注入リファクタ】ephemeral SQL pod。実プロセス
        // 起動(`current_exe()`)は`ProcessEphemeralRunner`が担い、GraphQL側
        // へは`Arc<dyn EphemeralRunner>`として注入する。`current_exe()`
        // 解決に失敗した場合(稀)は`None`のまま——resolver側が正直な
        // エラーメッセージを返す設計(既存の`topology`等と同じフォール
        // バック方針)。
        let ephemeral_for_graphql: Option<std::sync::Arc<dyn aruaru_dist::ephemeral::EphemeralRunner>> =
            match ephemeral_pod::ProcessEphemeralRunner::new() {
                Ok(runner) => Some(std::sync::Arc::new(runner)),
                Err(e) => {
                    tracing::warn!(error = %e, "ProcessEphemeralRunner::new() failed; ephemeralQuery will be unavailable");
                    None
                }
            };

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

        // 【2026-08-29 再設計 P3】APIキー自己発行の REST エンドポイント
        // (`POST /v1/keys/self-issue`)は撤廃。GraphQL の**認証不要 mutation**
        // `selfIssueKey`(`aruaru-graphql::VcsMutation`)へ移行済み。
        // 「認証無しで即発行できること自体が承認手続き」という性質は
        // mutation でもそのまま保てる。`KeyGuardian` は `build_schema` が
        // `AdminCtx.keyring` と同一インスタンスを schema data へ渡す。

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
                    topology: Some(topology_for_graphql.clone()),
                    schedule: Some(schedule_for_graphql.clone()),
                    federation: Some(federation_for_graphql.clone()),
                    parallel: Some(parallel_for_graphql.clone()),
                    keyring: Some(keyring_for_graphql.clone()),
                    object_table: Some(object_table_for_graphql.clone()),
                    closed_ts: Some(closed_ts_for_graphql.clone()),
                    wal_storage: Some(wal_storage_for_graphql.clone()),
                    sharded_store: Some(sharded_store_for_graphql.clone()),
                    ephemeral: ephemeral_for_graphql.clone(),
                    multi_raft: Some(multi_raft_for_graphql.clone()),
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
