//! 宣言的設定 `aruaru.yaml` のロードと表現。
//!
//! 設計の正本は [`docs/CONTROL_PLANE_REDESIGN.md`](../../../docs/CONTROL_PLANE_REDESIGN.md)。
//! 要点(P1 の範囲):
//!
//! - 運用設定は**宣言的ドキュメント**。実行時ミューテーション(`setX`)では
//!   なく「ファイルを書き換える → リロードが走る → 反映」。
//! - **静的セクション**(`server` / `raft`)の変更はプロセス再起動が必要
//!   (変更検知時に warn ログを出すだけ)。WunderGraph Cosmo と同じ制約。
//! - **動的セクション**(`query` / `backup` / `federation` / `follower_read`
//!   / `wal` / `sharded_store` / `disaster_backup`)は [`reconcile`] が
//!   差分を稼働中の `AdminState` へ適用する。
//! - `${VAR}` は環境変数へ展開する。
//!
//! P1 では読み込み・監視・reconcile(`backup.schedule` と
//! `federation.sources`)までを実装する。`query.parallel` 等の残りは
//! P2 で `AdminState` 側の型を4フィールド共有型へ寄せてから接続する。

pub mod reconcile;
pub mod watch;

use std::path::Path;

use serde::Deserialize;

pub use reconcile::reconcile;
pub use watch::spawn_config_watcher;

/// `aruaru.yaml` 全体。すべてのフィールドが `#[serde(default)]` で、
/// 部分的な設定ファイルでも読める(未指定は既定値)。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AruaruConfig {
    pub version: Option<String>,

    // ── 静的(再起動が必要) ──────────────────────────────
    pub server: ServerConfig,
    pub raft: RaftConfig,

    // ── 動的(ホットリロード対象) ────────────────────────
    pub query: QuerySection,
    pub backup: BackupSection,
    pub federation: FederationSection,
    pub follower_read: FollowerReadConfig,
    pub wal: WalConfig,
    pub sharded_store: ShardedStoreConfig,
    pub htap: HtapConfig,
    pub disaster_backup: DisasterBackupConfig,

    // ── コントロールプレーン取得(P5 で実配線) ──────────
    pub control_plane: ControlPlaneConfig,
    pub watch_config: WatchConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub data_dir: Option<String>,
    pub pg_port: Option<u16>,
    pub graphql_port: Option<u16>,
    pub metrics_addr: Option<String>,
    pub log_level: Option<String>,
    pub tls: TlsConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TlsConfig {
    pub cert: Option<String>,
    pub key: Option<String>,
    pub client_ca: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RaftConfig {
    pub node_id: Option<u64>,
    pub role: Option<String>,
    #[serde(default)]
    pub peers: Vec<String>,
    /// P2 で Raft 起動フローへ接続予定(現状は静的差分検知のみ)。
    #[serde(default)]
    #[allow(dead_code)]
    pub learner_peers: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct QuerySection {
    pub parallel: ParallelConfig,
}

/// 並列/分散クエリ実行設定。**GraphQL `ParallelConfigGql` と同じ4
/// フィールド**(ユーザー判断 2026-08-29: GraphQL の4フィールドを正とし
/// REST 側をここへ寄せる)。
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ParallelConfig {
    pub enabled: bool,
    pub max_workers: u32,
    pub chunk_size: u32,
    pub strategy: String,
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self { enabled: false, max_workers: 4, chunk_size: 10_000, strategy: "hash".into() }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BackupSection {
    pub schedule: BackupScheduleConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BackupScheduleConfig {
    pub enabled: bool,
    pub cron: String,
    pub kind: String,
}

impl Default for BackupScheduleConfig {
    fn default() -> Self {
        Self { enabled: false, cron: "0 3 * * *".into(), kind: "full".into() }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FederationSection {
    #[serde(default)]
    pub sources: Vec<FederationSourceConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FederationSourceConfig {
    pub name: String,
    pub kind: String,
    pub uri: String,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub pushdown: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FollowerReadConfig {
    pub target_lag_ms: u64,
    /// HLC クロックスキュー上限(ミリ秒、CockroachDB の `max_offset` 相当)。
    /// `0` = 無効(リモート HLC 値を常に受理、従来挙動)。
    pub max_offset_ms: u64,
}

impl Default for FollowerReadConfig {
    fn default() -> Self {
        Self { target_lag_ms: 3_000, max_offset_ms: 0 }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WalConfig {
    pub safekeepers: u32,
    pub quorum: u32,
}

impl Default for WalConfig {
    fn default() -> Self {
        Self { safekeepers: 3, quorum: 2 }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShardedStoreConfig {
    /// 0 = 論理コア数を自動採用。
    pub shards: usize,
}

/// HTAP(行→列非同期変換レプリカ)設定。正本は
/// `docs/CONTROL_PLANE_REDESIGN.md` 付録 A.7。**現状は静的セクション**
/// ——`columnar_replicas` / `read_consistency` は `--columnar-learner`
/// プロセスの起動可否・読み取り検証方式を決めるため稼働中の変更は
/// プロセス再起動が必要(`wal` / `sharded_store` と同じ扱い)。
/// `delta.compaction_threshold` も `ColumnarApplier` 構築時に渡すため静的。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct HtapConfig {
    /// テーブルへ列レプリカ(`--columnar-learner`)を持たせるか。
    pub columnar_replicas: bool,
    /// フォロワー(列レプリカ)読み取りの一貫性レベル。
    /// `"eventual"`(既定、非同期反映をそのまま読む)/
    /// `"raft-index"`(読み取りに必要な Raft index まで適用済みか検証。
    /// A.6-3)/ `"strict"`(raft-index + MVCC スナップショット分離)。
    pub read_consistency: String,
    pub delta: HtapDeltaConfig,
}

impl Default for HtapConfig {
    fn default() -> Self {
        Self {
            columnar_replicas: false,
            read_consistency: "eventual".into(),
            delta: HtapDeltaConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct HtapDeltaConfig {
    /// delta block がこの個数たまったら base へ compaction する
    /// (TiFlash DeltaTree の閾値 compaction、`ColumnarApplier`)。
    pub compaction_threshold: usize,
}

impl Default for HtapDeltaConfig {
    fn default() -> Self {
        Self { compaction_threshold: 8 }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DisasterBackupConfig {
    /// この宣言的設定を有効化するか（`feature = "disaster_email_backup"`
    /// ビルドでのみ意味を持つ）。P3 で reconcile 接続予定。
    #[allow(dead_code)]
    pub enabled: bool,
    #[allow(dead_code)]
    pub email: DisasterBackupEmail,
}

/// `open_raid_z_core::offsite_backup::EmailBackupTargetConfig` と同じ 7
/// フィールド（`aruaru.yaml` に正直に全項目を出す）。`${VAR}` 展開で
/// SMTP パスワードは環境変数名を渡す設計（値そのものは書かない）。
/// P3 で `config::reconcile` の feature ゲート付き分岐が消費する。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[allow(dead_code)]
pub struct DisasterBackupEmail {
    pub smtp_host: Option<String>,
    pub smtp_port: Option<u16>,
    pub smtp_username: Option<String>,
    /// SMTP パスワードが入っている**環境変数名**（値ではない）。
    pub smtp_password_env: Option<String>,
    pub from_address: Option<String>,
    pub to_address: Option<String>,
    #[serde(default)]
    pub allow_plaintext_for_testing: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ControlPlaneConfig {
    pub execution_config: ExecutionConfigSource,
    pub graph_api_token: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExecutionConfigSource {
    pub file: Option<String>,
    pub poll: ExecutionConfigPoll,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExecutionConfigPoll {
    pub url: Option<String>,
    /// P5(コントロールプレーン取得)で使用予定。
    #[serde(default = "default_poll_interval")]
    #[allow(dead_code)]
    pub interval_ms: u64,
}

fn default_poll_interval() -> u64 {
    10_000
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WatchConfig {
    pub enabled: bool,
    pub interval_ms: u64,
    pub startup_delay_ms: u64,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self { enabled: true, interval_ms: 2_000, startup_delay_ms: 500 }
    }
}

impl AruaruConfig {
    /// パスから読み込み、`${VAR}` を環境変数へ展開してから YAML を解析する。
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("設定ファイルを読めません ({}): {e}", path.display()))?;
        let expanded = expand_env(&raw);
        serde_norway::from_str(&expanded).map_err(|e| {
            anyhow::anyhow!("設定ファイルの YAML 解析に失敗しました ({}): {e}", path.display())
        })
    }
}

/// `${NAME}` を環境変数へ置換する。未定義の変数は空文字へ。
/// `$${` はリテラルの `${` としてエスケープできる。
pub fn expand_env(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'$' {
            // `$$` -> `$`(エスケープ)
            out.push('$');
            i += 2;
            continue;
        }
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            if let Some(end) = input[i + 2..].find('}') {
                let name = &input[i + 2..i + 2 + end];
                let val = std::env::var(name).unwrap_or_default();
                out.push_str(&val);
                i = i + 2 + end + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_yaml_is_all_defaults() {
        let c: AruaruConfig = serde_norway::from_str("{}").unwrap();
        assert_eq!(c.query.parallel.max_workers, 4);
        assert_eq!(c.query.parallel.strategy, "hash");
        assert!(!c.backup.schedule.enabled);
        assert_eq!(c.backup.schedule.cron, "0 3 * * *");
        assert_eq!(c.follower_read.target_lag_ms, 3_000);
        assert_eq!(c.wal.quorum, 2);
        assert!(c.federation.sources.is_empty());
        assert!(c.watch_config.enabled);
    }

    #[test]
    fn partial_yaml_overrides_only_named_fields() {
        let y = r#"
query:
  parallel:
    enabled: true
    max_workers: 16
backup:
  schedule:
    enabled: true
    cron: "*/5 * * * *"
    kind: "incremental"
federation:
  sources:
    - name: "warehouse"
      kind: "postgres"
      uri: "postgres://db/wh"
      read_only: true
"#;
        let c: AruaruConfig = serde_norway::from_str(y).unwrap();
        assert!(c.query.parallel.enabled);
        assert_eq!(c.query.parallel.max_workers, 16);
        assert_eq!(c.query.parallel.chunk_size, 10_000); // 未指定は既定
        assert_eq!(c.backup.schedule.cron, "*/5 * * * *");
        assert_eq!(c.federation.sources.len(), 1);
        assert_eq!(c.federation.sources[0].name, "warehouse");
        assert!(c.federation.sources[0].read_only);
        assert!(!c.federation.sources[0].pushdown);
    }

    #[test]
    fn env_expansion() {
        std::env::set_var("ARUARU_TEST_EXPAND_X", "sekret");
        assert_eq!(expand_env("a=${ARUARU_TEST_EXPAND_X}"), "a=sekret");
        assert_eq!(expand_env("undefined=${ARUARU_TEST_NOPE_ZZZ}"), "undefined=");
        assert_eq!(expand_env("literal $${NOT_EXPANDED}"), "literal ${NOT_EXPANDED}");
    }

    #[test]
    fn unknown_key_is_rejected() {
        let err = serde_norway::from_str::<AruaruConfig>("bogus_top_level: 1").unwrap_err();
        assert!(err.to_string().contains("bogus_top_level"), "{err}");
    }
}
