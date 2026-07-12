//! `open_raid_z_core` を実際に呼び出す `SnapshotBackend` 実装
//! (`open_raid_z` feature 有効時のみコンパイルされる)。
//!
//! `open_raid_z_core` は別リポジトリ・別 Cargo ワークスペース
//! (`open-raid-z/open_runo_zfs_source/open_raid_z_core`)であり、本クレートは
//! `default-features = false`(WinFsp/dxc/Windows SDK 不要のCPUフォール
//! バックのみ)で path 依存する。これにより、このバックエンドは Windows
//! 実機の WinFsp SDK が無い CI/開発環境でも実際の RAID-Z プール上で検証
//! 可能(実マウントは行わず、`FileBackedDevice` によるファイルI/Oのみ)。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use open_raid_z_core::block_device::FileBackedDevice;
use open_raid_z_core::pool::Pool;
use open_raid_z_core::vdev::{RaidLevel, RaidZVdev};

use crate::snapshot_pairing::SnapshotBackend;

/// `aruaru-db` の単一データセットを載せる、テスト・開発向けの最小構成
/// RAID-Z2 プール(6デバイス)をバックエンドとするスナップショットバックエンド。
pub struct OpenRaidZSnapshotBackend {
    dataset_name: String,
    pool: Mutex<Pool<RaidZVdev<FileBackedDevice>>>,
}

impl OpenRaidZSnapshotBackend {
    /// `dir` 配下に固定サイズのファイルバック仮想ディスクを6枚作成し、
    /// RAID-Z2 プールを構築、`dataset_name` という単一データセットを
    /// 用意した状態で返す。
    pub fn new(dir: &Path, dataset_name: &str) -> std::io::Result<Self> {
        const CHUNK_SIZE: usize = 4096;
        const NUM_STRIPES: u64 = 256;
        std::fs::create_dir_all(dir)?;
        let devices: Vec<FileBackedDevice> = (0..6)
            .map(|i| {
                let path: PathBuf = dir.join(format!("disk{i}.img"));
                FileBackedDevice::create_fixed_size(&path, CHUNK_SIZE as u64 * NUM_STRIPES)
            })
            .collect::<Result<_, _>>()
            .map_err(std::io::Error::other)?;
        let vdev = RaidZVdev::new(devices, RaidLevel::Z2, CHUNK_SIZE);
        let mut pool = Pool::new(vdev, NUM_STRIPES);
        pool.create_dataset(dataset_name)
            .map_err(std::io::Error::other)?;
        Ok(Self {
            dataset_name: dataset_name.to_string(),
            pool: Mutex::new(pool),
        })
    }

    /// テスト検証用: 現時点でプールに存在するスナップショット名一覧。
    pub fn snapshot_names(&self) -> Vec<String> {
        self.pool.lock().unwrap().snapshot_names(&self.dataset_name)
    }
}

impl SnapshotBackend for OpenRaidZSnapshotBackend {
    fn snapshot(&self, label: &str) -> Result<String, String> {
        let mut pool = self.pool.lock().map_err(|e| e.to_string())?;
        pool.create_snapshot(&self.dataset_name, label)
            .map_err(|e| format!("{e:?}"))?;
        Ok(label.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raft::{Applier, Command, CommandResponse, RaftNode};
    use crate::snapshot_pairing::{wire_to_node, SnapshotPairingRegistry};
    use std::sync::Arc;

    struct NoopApplier;
    impl Applier for NoopApplier {
        fn apply(&self, _command: &Command) -> CommandResponse {
            CommandResponse::ok()
        }
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("aruaru_raidz_snap_pairing_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// 実際のRaft commitが実際のRAID-Zプールのスナップショット作成を
    /// トリガーし、対応関係が問い合わせ可能であることをend-to-endで検証する。
    #[test]
    fn real_raft_commit_triggers_real_raid_z_snapshot() {
        let dir = scratch_dir("e2e");
        let backend = Arc::new(
            OpenRaidZSnapshotBackend::new(&dir, "aruaru-db").expect("pool setup"),
        );
        let registry = Arc::new(SnapshotPairingRegistry::new(backend.clone()));
        let node = Arc::new(RaftNode::new(1, NoopApplier, vec![]));
        wire_to_node(&node, registry.clone());

        node.become_leader();
        node.propose(&Command::Exec("INSERT 1".into())).unwrap();
        node.try_commit_to(1);
        assert_eq!(node.apply_committed(), 1);

        // レジストリ経由の問い合わせと、実プールの実スナップショット一覧の
        // 両方で確認する。
        assert_eq!(registry.snapshot_for_commit(1), Some("commit-1".to_string()));
        assert_eq!(backend.snapshot_names(), vec!["commit-1".to_string()]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
