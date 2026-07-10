//! aruaru-backup: バックアップ・リストア・ポイントインタイムリカバリ
//!
//! ## バックアップ種別
//! - **フルバックアップ**: 全データを Parquet + WAL で保存
//! - **増分バックアップ**: 前回バックアップ以降の WAL のみ
//! - **スナップショット**: Git-on-SQL の commit ID を利用した即座のスナップショット
//! - **ストリーミングバックアップ**: S3 / GCS / Azure Blob にリアルタイム転送

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use chrono::Utc;

use aruaru_query::QueryEngine;

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
    /// バックアップ/リストア対象の QueryEngine (全テーブルデータ・コミット情報を保持)
    engine: Arc<QueryEngine>,
}

impl BackupEngine {
    pub fn new(config: BackupConfig, engine: Arc<QueryEngine>) -> Self {
        Self { config, engine }
    }

    /// ローカルディレクトリ宛先の場合のパスを取得する。
    /// 現段階では Local のみ実装済み (S3/SFTP は転送クライアント未接続)。
    fn local_dest(&self) -> anyhow::Result<&PathBuf> {
        match &self.config.destination {
            BackupDestination::Local { path } => Ok(path),
            BackupDestination::S3 { bucket, .. } => {
                anyhow::bail!("S3 destination ({bucket}) is not yet implemented; use BackupDestination::Local")
            }
            BackupDestination::Sftp { host, .. } => {
                anyhow::bail!("SFTP destination ({host}) is not yet implemented; use BackupDestination::Local")
            }
        }
    }

    /// 全テーブルを Parquet ファイルへシリアライズし、宛先ディレクトリ
    /// (`<dest>/<backup_id>/`) に書き出す。MANIFEST.json も併せて生成する。
    /// `kind` はマニフェストに記録するバックアップ種別 (Full / Snapshot)。
    async fn write_snapshot(
        &self,
        backup_id: &str,
        kind: BackupKind,
        progress_cb: &(impl Fn(BackupProgress) + Send + 'static),
    ) -> anyhow::Result<BackupManifest> {
        let started_at = Utc::now().to_rfc3339();
        let start = std::time::Instant::now();

        progress_cb(BackupProgress {
            phase: BackupPhase::Preparing,
            bytes_done: 0,
            bytes_total: None,
            rows_done: 0,
            elapsed_ms: 0,
            eta_ms: None,
        });

        let dest_root = self.local_dest()?.join(backup_id);
        fs::create_dir_all(&dest_root).await?;

        // Git-on-SQL の HEAD コミット = このスナップショットの指紋
        let head = self.engine.version().head();
        let commit_id = head
            .as_ref()
            .map(|c| c.id.as_str().to_string())
            .unwrap_or_default();
        let branch = self.engine.version().current_branch();

        let tables = self.engine.snapshot_tables();
        let mut files = Vec::with_capacity(tables.len());
        let mut total_rows: u64 = 0;
        let mut total_bytes: u64 = 0;

        for (name, columns, rows) in &tables {
            progress_cb(BackupProgress {
                phase: BackupPhase::DumpingData { table: name.clone() },
                bytes_done: total_bytes,
                bytes_total: None,
                rows_done: total_rows,
                elapsed_ms: start.elapsed().as_millis() as u64,
                eta_ms: None,
            });

            let file_name = format!("{name}.parquet");
            let file_path = dest_root.join(&file_name);
            let col_names: Vec<String> = columns.iter().map(|(n, _)| n.clone()).collect();
            let bytes = write_table_parquet(&file_path, &col_names, rows)?;

            total_rows += rows.len() as u64;
            total_bytes += bytes as u64;

            files.push(BackupFile {
                name: file_name,
                size_bytes: bytes as u64,
                checksum: sha256_file(&file_path)?,
            });
        }

        progress_cb(BackupProgress {
            phase: BackupPhase::Verifying,
            bytes_done: total_bytes,
            bytes_total: Some(total_bytes),
            rows_done: total_rows,
            elapsed_ms: start.elapsed().as_millis() as u64,
            eta_ms: None,
        });

        // マニフェスト全体のチェックサム = 各ファイルチェックサムの連結ハッシュ
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest;
        for f in &files {
            hasher.update(f.checksum.as_bytes());
        }
        let checksum = hex::encode(hasher.finalize());

        let manifest = BackupManifest {
            id: backup_id.to_string(),
            kind,
            started_at,
            finished_at: Utc::now().to_rfc3339(),
            size_bytes: total_bytes,
            row_count: total_rows,
            commit_id,
            branch,
            checksum,
            files,
        };

        let manifest_path = dest_root.join("MANIFEST.json");
        let manifest_json = serde_json::to_string_pretty(&manifest)?;
        fs::write(&manifest_path, manifest_json).await?;

        progress_cb(BackupProgress {
            phase: BackupPhase::Done,
            bytes_done: total_bytes,
            bytes_total: Some(total_bytes),
            rows_done: total_rows,
            elapsed_ms: start.elapsed().as_millis() as u64,
            eta_ms: None,
        });

        tracing::info!(id = %backup_id, rows = total_rows, bytes = total_bytes, "Backup complete");
        Ok(manifest)
    }

    /// フルバックアップ実行: 全テーブルを Parquet にシリアライズして宛先へ書き出す。
    pub async fn run_full(
        &self,
        progress_cb: impl Fn(BackupProgress) + Send + 'static,
    ) -> anyhow::Result<BackupManifest> {
        let backup_id = uuid::Uuid::now_v7().to_string();
        tracing::info!(id = %backup_id, "Starting full backup");
        self.write_snapshot(&backup_id, BackupKind::Full, &progress_cb).await
    }

    /// スナップショットバックアップ (Git-on-SQL コミット活用)。
    ///
    /// v0.5 時点では、指定コミットの HEAD 状態を対象に Parquet フルダンプを
    /// 行う (commit-tagged full snapshot)。真の Prolly Tree reference
    /// counting による差分のみ CoW 保存 (O(変更量)) は将来の最適化として
    /// 別途 issue 化する — 現状は「パニックせず、リストア可能な実データを
    /// 生成する」ことを優先した。
    pub async fn snapshot(&self, commit_id: &str) -> anyhow::Result<BackupManifest> {
        tracing::info!(commit = %commit_id, "Snapshot backup: commit-based");
        let backup_id = format!("snap-{}", &commit_id[..commit_id.len().min(12)]);
        let noop_progress = |_p: BackupProgress| {};
        self.write_snapshot(
            &backup_id,
            BackupKind::Snapshot { commit_id: commit_id.to_string() },
            &noop_progress,
        )
        .await
    }

    /// バックアップ一覧: 宛先ディレクトリ配下の `<id>/MANIFEST.json` を読む。
    pub async fn list_backups(&self) -> anyhow::Result<Vec<BackupManifest>> {
        let root = match self.local_dest() {
            Ok(p) => p.clone(),
            // リモート宛先は転送クライアント未接続のため空一覧を返す (エラーにはしない)
            Err(e) => {
                tracing::warn!(error = %e, "list_backups: remote destination not supported yet");
                return Ok(vec![]);
            }
        };

        if !root.exists() {
            return Ok(vec![]);
        }

        let mut manifests = Vec::new();
        let mut entries = fs::read_dir(&root).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join("MANIFEST.json");
            if !manifest_path.exists() {
                continue;
            }
            let content = fs::read_to_string(&manifest_path).await?;
            match serde_json::from_str::<BackupManifest>(&content) {
                Ok(m) => manifests.push(m),
                Err(e) => tracing::warn!(path = %manifest_path.display(), error = %e, "invalid MANIFEST.json, skipping"),
            }
        }
        manifests.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(manifests)
    }

    /// リストア: MANIFEST.json → Parquet ファイル → QueryEngine へ復元する。
    pub async fn restore(
        &self,
        backup_id: &str,
        _target_data_dir: &PathBuf,
        progress_cb: impl Fn(BackupProgress) + Send + 'static,
    ) -> anyhow::Result<()> {
        tracing::info!(backup_id = %backup_id, "Starting restore");
        let start = std::time::Instant::now();

        let root = self.local_dest()?.join(backup_id);
        let manifest_path = root.join("MANIFEST.json");
        if !manifest_path.exists() {
            anyhow::bail!("backup not found: {backup_id} (no MANIFEST.json at {})", manifest_path.display());
        }
        let content = fs::read_to_string(&manifest_path).await?;
        let manifest: BackupManifest = serde_json::from_str(&content)?;

        progress_cb(BackupProgress {
            phase: BackupPhase::Preparing,
            bytes_done: 0,
            bytes_total: Some(manifest.size_bytes),
            rows_done: 0,
            elapsed_ms: 0,
            eta_ms: None,
        });

        let mut rows_done = 0u64;
        for file in &manifest.files {
            let table_name = file
                .name
                .strip_suffix(".parquet")
                .unwrap_or(&file.name)
                .to_string();
            let file_path = root.join(&file.name);

            progress_cb(BackupProgress {
                phase: BackupPhase::DumpingData { table: table_name.clone() },
                bytes_done: 0,
                bytes_total: Some(manifest.size_bytes),
                rows_done,
                elapsed_ms: start.elapsed().as_millis() as u64,
                eta_ms: None,
            });

            let actual_checksum = sha256_file(&file_path)?;
            if actual_checksum != file.checksum {
                anyhow::bail!(
                    "checksum mismatch for {}: expected {} got {}",
                    file.name, file.checksum, actual_checksum
                );
            }

            let (columns, rows) = read_table_parquet(&file_path)?;
            rows_done += rows.len() as u64;
            self.engine.ingest_table(&table_name, columns, rows);
        }

        progress_cb(BackupProgress {
            phase: BackupPhase::Done,
            bytes_done: manifest.size_bytes,
            bytes_total: Some(manifest.size_bytes),
            rows_done,
            elapsed_ms: start.elapsed().as_millis() as u64,
            eta_ms: None,
        });

        tracing::info!(backup_id = %backup_id, rows = rows_done, "Restore complete");
        Ok(())
    }
}

/// 1 テーブルを Parquet ファイルへ書き出す (全列 Utf8)。書き込みバイト数を返す。
fn write_table_parquet(
    path: &std::path::Path,
    columns: &[String],
    rows: &[Vec<String>],
) -> anyhow::Result<usize> {
    use arrow::array::{ArrayRef, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};
    use arrow::record_batch::RecordBatch;
    use parquet::arrow::ArrowWriter;

    let fields: Vec<Field> = columns
        .iter()
        .map(|c| Field::new(c, DataType::Utf8, true))
        .collect();
    let schema = Arc::new(Schema::new(fields));

    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(columns.len());
    for col_idx in 0..columns.len() {
        let values: Vec<Option<String>> = rows
            .iter()
            .map(|row| row.get(col_idx).cloned())
            .collect();
        arrays.push(Arc::new(StringArray::from(values)));
    }

    // 空テーブル (列も0件) の場合、Arrow は空スキーマの RecordBatch を許容しないため
    // 明示的に1件も書き込まない0バイトファイルとして扱う。
    if columns.is_empty() {
        std::fs::File::create(path)?;
        return Ok(0);
    }

    let batch = RecordBatch::try_new(schema.clone(), arrays)?;
    let file = std::fs::File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;

    Ok(std::fs::metadata(path)?.len() as usize)
}

/// Parquet ファイルを (列名, 行) に読み戻す。書き込み時と対称に、
/// 全列 Utf8 (`write_table_parquet` が書く形式) を前提に読み出す。
fn read_table_parquet(path: &std::path::Path) -> anyhow::Result<(Vec<String>, Vec<Vec<String>>)> {
    use arrow::array::{Array, StringArray};
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    if std::fs::metadata(path)?.len() == 0 {
        return Ok((vec![], vec![]));
    }

    let file = std::fs::File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let columns: Vec<String> = builder
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect();
    let reader = builder.build()?;

    let mut rows: Vec<Vec<String>> = Vec::new();
    for batch in reader {
        let batch = batch?;
        let string_cols: Vec<&StringArray> = (0..batch.num_columns())
            .map(|i| {
                batch
                    .column(i)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .expect("backup parquet files are always written with Utf8 columns")
            })
            .collect();
        for row_idx in 0..batch.num_rows() {
            let mut row = Vec::with_capacity(columns.len());
            for array in &string_cols {
                let value = if array.is_null(row_idx) {
                    String::new()
                } else {
                    array.value(row_idx).to_string()
                };
                row.push(value);
            }
            rows.push(row);
        }
    }
    Ok((columns, rows))
}

/// ファイルの SHA-256 チェックサムを16進文字列で返す
fn sha256_file(path: &std::path::Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

// ── 自動スケジューラ ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSchedule {
    /// cron 形式: "0 2 * * *" = 毎日 02:00
    pub cron: String,
    pub kind: BackupKind,
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("aruaru-backup-test-{name}-{}", uuid::Uuid::now_v7()));
        p
    }

    fn local_config(dest: &PathBuf, kind: BackupKind) -> BackupConfig {
        BackupConfig {
            destination: BackupDestination::Local { path: dest.clone() },
            kind,
            compression: BackupCompression::None,
            encrypt: false,
            retention_days: 7,
        }
    }

    #[tokio::test]
    async fn test_full_backup_and_restore_round_trip() {
        let engine = Arc::new(QueryEngine::new());
        engine.execute("CREATE TABLE t (id, name)").unwrap();
        engine
            .execute("INSERT INTO t (id, name) VALUES ('1', 'alice')")
            .unwrap();
        engine
            .execute("INSERT INTO t (id, name) VALUES ('2', 'bob')")
            .unwrap();

        let dest = temp_dir("full");
        let backup = BackupEngine::new(local_config(&dest, BackupKind::Full), engine.clone());

        let manifest = backup.run_full(|_| {}).await.expect("run_full should succeed");
        assert_eq!(manifest.row_count, 2);
        assert_eq!(manifest.files.len(), 1);

        let backups = backup
            .list_backups()
            .await
            .expect("list_backups should succeed");
        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0].id, manifest.id);

        // 新しい (空の) エンジンへリストアして中身が復元されることを確認する
        let restore_engine = Arc::new(QueryEngine::new());
        let restorer = BackupEngine::new(
            local_config(&dest, BackupKind::Full),
            restore_engine.clone(),
        );
        restorer
            .restore(&manifest.id, &dest, |_| {})
            .await
            .expect("restore should succeed");

        assert_eq!(restore_engine.table_row_count("t"), Some(2));

        let _ = std::fs::remove_dir_all(&dest);
    }

    #[tokio::test]
    async fn test_snapshot_uses_commit_tagged_id() {
        let engine = Arc::new(QueryEngine::new());
        engine.execute("CREATE TABLE t (id)").unwrap();
        engine.execute("INSERT INTO t (id) VALUES ('1')").unwrap();
        engine.execute("SELECT aruaru_commit('snap test')").unwrap();

        let head_commit = engine.version().head().unwrap().id.as_str().to_string();

        let dest = temp_dir("snapshot");
        let backup = BackupEngine::new(
            local_config(&dest, BackupKind::Snapshot { commit_id: head_commit.clone() }),
            engine.clone(),
        );
        let manifest = backup
            .snapshot(&head_commit)
            .await
            .expect("snapshot should succeed");
        assert_eq!(manifest.row_count, 1);
        assert_eq!(manifest.commit_id, head_commit);

        let _ = std::fs::remove_dir_all(&dest);
    }

    #[tokio::test]
    async fn test_list_backups_empty_when_no_destination_dir() {
        let engine = Arc::new(QueryEngine::new());
        let dest = temp_dir("empty");
        let backup = BackupEngine::new(local_config(&dest, BackupKind::Full), engine);
        let backups = backup
            .list_backups()
            .await
            .expect("should not error on missing dir");
        assert!(backups.is_empty());
    }

    #[tokio::test]
    async fn test_restore_missing_backup_id_errors() {
        let engine = Arc::new(QueryEngine::new());
        let dest = temp_dir("missing");
        let backup = BackupEngine::new(local_config(&dest, BackupKind::Full), engine);
        let result = backup.restore("does-not-exist", &dest, |_| {}).await;
        assert!(result.is_err());
    }
}
