//! aruaru-backup: バックアップ・リストア・ポイントインタイムリカバリ
//!
//! ## バックアップ種別
//! - **フルバックアップ**: 全データを Parquet + WAL で保存
//! - **増分バックアップ**: 前回バックアップ以降の WAL のみ
//! - **スナップショット**: Git-on-SQL の commit ID を利用した即座のスナップショット
//! - **ストリーミングバックアップ**: S3 / GCS / Azure Blob にリアルタイム転送

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;
use chrono::Utc;

// ── バックアップ設定 ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    /// バックアップ先
    pub destination: BackupDestination,
    /// バックアップ種別
    pub kind: BackupKind,
    /// 圧縮
    pub compression: BackupCompression,
    /// 暗号化 (AES-256-GCM)
    pub encrypt: bool,
    /// 保持期間 (日)
    pub retention_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackupDestination {
    /// ローカルディレクトリ
    Local { path: PathBuf },
    /// S3 互換 (AWS / MinIO / R2)
    S3 {
        bucket: String,
        prefix: String,
        region: String,
        endpoint: Option<String>,
    },
    /// SSH/SFTP リモート
    Sftp {
        host: String,
        port: u16,
        path: String,
        username: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BackupKind {
    Full,
    Incremental,
    Snapshot { commit_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum BackupCompression {
    #[default]
    Zstd,
    Gzip,
    None,
}

// ── バックアップメタデータ ────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub id: String,
    pub kind: BackupKind,
    pub started_at: String,
    pub finished_at: String,
    pub size_bytes: u64,
    pub row_count: u64,
    pub commit_id: String,
    pub branch: String,
    pub checksum: String,
    pub files: Vec<BackupFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupFile {
    pub name: String,
    pub size_bytes: u64,
    pub checksum: String,
}

// ── バックアップ進捗 ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupProgress {
    pub phase: BackupPhase,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub rows_done: u64,
    pub elapsed_ms: u64,
    pub eta_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BackupPhase {
    Preparing,
    DumpingSchema,
    DumpingData { table: String },
    DumpingWal,
    Compressing,
    Uploading,
    Verifying,
    Done,
    Failed(String),
}

// ── バックアップエンジン ─────────────────────────────────────

pub struct BackupEngine {
    config: BackupConfig,
}

impl BackupEngine {
    pub fn new(config: BackupConfig) -> Self {
        Self { config }
    }

    /// フルバックアップ実行
    pub async fn run_full(
        &self,
        progress_cb: impl Fn(BackupProgress) + Send + 'static,
    ) -> anyhow::Result<BackupManifest> {
        let started_at = Utc::now().to_rfc3339();
        let backup_id = uuid::Uuid::now_v7().to_string();

        tracing::info!(id = %backup_id, "Starting full backup");

        progress_cb(BackupProgress {
            phase: BackupPhase::Preparing,
            bytes_done: 0,
            bytes_total: None,
            rows_done: 0,
            elapsed_ms: 0,
            eta_ms: None,
        });

        // TODO:
        // 1. aruaru-core::Engine から Prolly Tree root_hash 取得 (スナップショット点)
        // 2. 全テーブルを Parquet ファイルにシリアライズ
        // 3. WAL の最新位置を記録
        // 4. BackupDestination に転送
        // 5. MANIFEST.json を生成・転送

        progress_cb(BackupProgress {
            phase: BackupPhase::Done,
            bytes_done: 0,
            bytes_total: Some(0),
            rows_done: 0,
            elapsed_ms: 0,
            eta_ms: None,
        });

        Ok(BackupManifest {
            id: backup_id,
            kind: BackupKind::Full,
            started_at,
            finished_at: Utc::now().to_rfc3339(),
            size_bytes: 0,
            row_count: 0,
            commit_id: "TODO".to_string(),
            branch: "main".to_string(),
            checksum: "TODO".to_string(),
            files: vec![],
        })
    }

    /// スナップショットバックアップ (Git-on-SQL コミット活用)
    pub async fn snapshot(&self, commit_id: &str) -> anyhow::Result<BackupManifest> {
        tracing::info!(commit = %commit_id, "Snapshot backup: commit-based");
        // Git-on-SQL の Prolly Tree ルートハッシュ = 完全な状態の指紋
        // → コピーオンライトで O(変更量) のスナップショットが作れる
        // TODO: Prolly Tree の reference counting で実装
        todo!("Snapshot backup via Prolly Tree CoW")
    }

    /// バックアップ一覧
    pub async fn list_backups(&self) -> anyhow::Result<Vec<BackupManifest>> {
        // TODO: destination から MANIFEST.json ファイル一覧を取得
        Ok(vec![])
    }

    /// リストア
    pub async fn restore(
        &self,
        backup_id: &str,
        target_data_dir: &PathBuf,
        progress_cb: impl Fn(BackupProgress) + Send + 'static,
    ) -> anyhow::Result<()> {
        tracing::info!(backup_id = %backup_id, "Starting restore");
        // TODO: MANIFEST.json → Parquet ファイル → fjall に復元
        todo!("Restore from backup")
    }
}

// ── 自動スケジューラ ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSchedule {
    /// cron 形式: "0 2 * * *" = 毎日 02:00
    pub cron: String,
    pub kind: BackupKind,
    pub enabled: bool,
}
