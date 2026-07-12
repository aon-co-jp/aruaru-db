//! 永続ストレージ (fjall LSM)
//!
//! QueryEngine のインメモリテーブルを fjall キースペースに永続化し、
//! 再起動時に復元する。fjall は LSM ツリーの組み込みストレージエンジン。
//!
//! ## レイアウト
//! - パーティション `__meta` : `table 名 → スキーマ(JSON)`
//! - パーティション `__data` : `"{table}\0{pk}" → 行(JSON 配列)`
//!
//! コミット時など明示的な `persist()` で WAL を同期する。

use fjall::{Config, Keyspace, PartitionCreateOptions, PartitionHandle, PersistMode};
use serde::{Deserialize, Serialize};

use crate::catalog::ColumnType;

// ── ZFS互換チェックサム (§0.6 ハイブリッド: ZFS互換 + ACID互換) ─────────────
//
// open-raid-z (`open_raid_z_core::checksum`) と**アルゴリズム・型ともに同一**
// の SHA-256 チェックサム。「全書き込みデータにチェックサムを付与し、
// 読み込み時に検証する」というZFSのデータ整合性モデルを、aruaru-db の
// 既存ACIDトランザクション層(BEGIN/COMMIT/ROLLBACK、Git-on-SQLコミット)の
// 上に追加する。二重化ではなく直交する保証: ACIDが「正しい順序で確定する」
// ことを保証し、チェックサムが「保存されたバイトが破損していない」ことを
// 保証する。open-raid-z 側で checksum.rs のアルゴリズムが変わった場合は
// こちらも追随させ、2つのプロジェクト間でチェックサム値が常に相互検証可能
// であるようにすること(§0.5 の「共有すべき中心技術」の実例)。

/// open-raid-z の `open_raid_z_core::checksum::Checksum` と同一の型。
pub type Checksum = [u8; 32];

/// データのチェックサムを計算する(SHA-256)。open-raid-z の
/// `compute_checksum` とバイト単位で同一の実装・同一の出力を返す。
pub fn compute_checksum(data: &[u8]) -> Checksum {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

#[cfg(test)]
mod checksum_tests {
    use super::*;

    #[test]
    fn same_data_produces_same_checksum() {
        assert_eq!(compute_checksum(b"hello"), compute_checksum(b"hello"));
    }

    #[test]
    fn different_data_produces_different_checksum() {
        assert_ne!(compute_checksum(b"hello"), compute_checksum(b"hellp"));
    }

    #[test]
    fn checksum_detects_single_bit_flip() {
        // open-raid-z's checksum_self_healing test scenario, reproduced
        // here so aruaru-db's checksum layer is verified against the
        // exact same bit-rot scenario it's meant to detect.
        let original = vec![0xAAu8; 64];
        let mut corrupted = original.clone();
        corrupted[30] ^= 0x01;
        assert_ne!(compute_checksum(&original), compute_checksum(&corrupted));
    }
}


/// ストレージエラー
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("fjall error: {0}")]
    Fjall(#[from] fjall::Error),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    /// ZFS互換チェックサム検証エラー: 保存時のチェックサムと読み込み時に
    /// 再計算したチェックサムが一致しない(ビットロット等でデータが破損)。
    /// ACIDトランザクション層が保証する「正しい順序で確定した」ことと、
    /// このエラーが保証する「保存後にバイトが破損していない」ことは別軸。
    #[error("checksum mismatch for {table}/{pk:?}: stored data does not match its checksum (possible corruption)")]
    ChecksumMismatch { table: String, pk: Vec<u8> },
}

type Result<T> = std::result::Result<T, StorageError>;

/// テーブルスキーマ (永続化用)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSchema {
    pub table: String,
    pub columns: Vec<(String, ColumnType)>,
}

/// 永続ストア
pub struct PersistentStore {
    keyspace: Keyspace,
    meta: PartitionHandle,
    data: PartitionHandle,
    /// ZFS互換チェックサム: `{table}\0{pk}` → SHA-256(行のJSONバイト列)。
    /// `data` パーティションと1対1対応。別パーティションに分けているのは
    /// 本物のZFSがチェックサムをデータブロックと別のブロックポインタ木に
    /// 格納するのと同じ理由(チェックサム自体の破損とデータの破損を
    /// 独立して検出できるようにするため)。
    checksums: PartitionHandle,
}

impl PersistentStore {
    /// 指定パスにキースペースを開く (なければ作成)
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let keyspace = Config::new(path).open()?;
        let meta = keyspace.open_partition("__meta", PartitionCreateOptions::default())?;
        let data = keyspace.open_partition("__data", PartitionCreateOptions::default())?;
        let checksums = keyspace.open_partition("__checksums", PartitionCreateOptions::default())?;
        Ok(Self {
            keyspace,
            meta,
            data,
            checksums,
        })
    }

    /// データ行キー: `{table}\0{pk}`
    fn data_key(table: &str, pk: &[u8]) -> Vec<u8> {
        let mut k = table.as_bytes().to_vec();
        k.push(0);
        k.extend_from_slice(pk);
        k
    }

    /// スキーマを保存
    pub fn save_schema(&self, table: &str, columns: &[(String, ColumnType)]) -> Result<()> {
        let schema = StoredSchema {
            table: table.to_string(),
            columns: columns.to_vec(),
        };
        let json = serde_json::to_vec(&schema)?;
        self.meta.insert(table.as_bytes(), json)?;
        Ok(())
    }

    /// 全スキーマを読み出す
    pub fn load_schemas(&self) -> Result<Vec<StoredSchema>> {
        let mut out = Vec::new();
        for kv in self.meta.iter() {
            let (_k, v) = kv?;
            let schema: StoredSchema = serde_json::from_slice(&v)?;
            out.push(schema);
        }
        Ok(out)
    }

    /// 1 行を保存 (pk と行の文字列配列)。書き込みバイト列のSHA-256
    /// チェックサムも同時に記録する(ZFS互換: 書き込み時に必ずチェックサムを
    /// 付与)。
    pub fn save_row(&self, table: &str, pk: &[u8], row: &[String]) -> Result<()> {
        let key = Self::data_key(table, pk);
        let json = serde_json::to_vec(row)?;
        let checksum = compute_checksum(&json);
        self.data.insert(&key, json)?;
        self.checksums.insert(&key, checksum.to_vec())?;
        Ok(())
    }

    /// 1 行を削除 (対応するチェックサムも併せて削除する)
    pub fn delete_row(&self, table: &str, pk: &[u8]) -> Result<()> {
        let key = Self::data_key(table, pk);
        self.data.remove(&key)?;
        self.checksums.remove(&key)?;
        Ok(())
    }

    /// テーブルの全行を走査 (pk, 行)。読み込みごとにチェックサムを再計算し、
    /// 保存時の値と照合する(ZFS互換: 読み込み時に必ず検証する)。不一致は
    /// `StorageError::ChecksumMismatch` として返す(黙って壊れたデータを
    /// 返さない)。チェックサム自体が(何らかの理由で)見つからない場合は
    /// 検証をスキップする(このパーティションが導入される前に書かれた
    /// 既存データとの後方互換のため、エラーにはしない)。
    pub fn scan_table(&self, table: &str) -> Result<Vec<(Vec<u8>, Vec<String>)>> {
        let mut prefix = table.as_bytes().to_vec();
        prefix.push(0);
        let plen = prefix.len();

        let mut out = Vec::new();
        for kv in self.data.prefix(prefix.clone()) {
            let (k, v) = kv?;
            let pk = k[plen..].to_vec();

            if let Some(stored_checksum) = self.checksums.get(&k)? {
                let actual = compute_checksum(&v);
                if actual.as_slice() != stored_checksum.as_ref() {
                    return Err(StorageError::ChecksumMismatch { table: table.to_string(), pk });
                }
            }

            let row: Vec<String> = serde_json::from_slice(&v)?;
            out.push((pk, row));
        }
        Ok(out)
    }

    /// **スクラブ**(ZFS互換): 全テーブルの全行のチェックサムを検証し、
    /// 破損が見つかった行の一覧を返す(パニックしない・止まらない —
    /// `scan_table`と違い、最初の不一致で打ち切らず全件チェックする)。
    /// 運用者が定期的に呼び出して「保存データがまだ健全か」を能動的に
    /// 確認するための機能で、ZFSの`zpool scrub`に相当する。
    pub fn scrub(&self) -> Result<Vec<(String, Vec<u8>)>> {
        let mut corrupted = Vec::new();
        for kv in self.data.iter() {
            let (k, v) = kv?;
            let Some(stored_checksum) = self.checksums.get(&k)? else {
                continue; // チェックサム未記録の既存データはスキップ (後方互換)
            };
            let actual = compute_checksum(&v);
            if actual.as_slice() != stored_checksum.as_ref() {
                // キーを `{table}\0{pk}` から分離してレポートする
                if let Some(nul_pos) = k.iter().position(|&b| b == 0) {
                    let table = String::from_utf8_lossy(&k[..nul_pos]).into_owned();
                    let pk = k[nul_pos + 1..].to_vec();
                    corrupted.push((table, pk));
                }
            }
        }
        Ok(corrupted)
    }

    /// テーブルを丸ごと削除 (スキーマ + 全行 + 対応するチェックサム)
    pub fn drop_table(&self, table: &str) -> Result<()> {
        // スキーマ削除
        self.meta.remove(table.as_bytes())?;
        // データ行を prefix 走査して全削除 (チェックサムも同じキーで併せて削除)
        let mut prefix = table.as_bytes().to_vec();
        prefix.push(0);
        let keys: Vec<Vec<u8>> = self
            .data
            .prefix(prefix)
            .filter_map(|kv| kv.ok().map(|(k, _)| k.to_vec()))
            .collect();
        for k in keys {
            self.data.remove(&k)?;
            self.checksums.remove(&k)?;
        }
        Ok(())
    }

    /// WAL を同期して永続化を確定する
    pub fn persist(&self) -> Result<()> {
        self.keyspace.persist(PersistMode::SyncAll)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_persist_and_reload() {
        let dir = std::env::temp_dir().join(format!("aruaru-fjall-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        {
            let store = PersistentStore::open(&dir).unwrap();
            store
                .save_schema(
                    "users",
                    &[
                        ("id".to_string(), ColumnType::BigInt),
                        ("name".to_string(), ColumnType::Text),
                    ],
                )
                .unwrap();
            store.save_row("users", b"1", &["1".into(), "Alice".into()]).unwrap();
            store.save_row("users", b"2", &["2".into(), "Bob".into()]).unwrap();
            store.persist().unwrap();
        }

        // 再オープンして復元を確認
        {
            let store = PersistentStore::open(&dir).unwrap();
            let schemas = store.load_schemas().unwrap();
            assert_eq!(schemas.len(), 1);
            assert_eq!(schemas[0].table, "users");

            let rows = store.scan_table("users").unwrap();
            assert_eq!(rows.len(), 2);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_table_detects_corrupted_row() {
        let dir = std::env::temp_dir().join(format!("aruaru-fjall-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let store = PersistentStore::open(&dir).unwrap();
        store
            .save_schema("users", &[("id".to_string(), ColumnType::BigInt), ("name".to_string(), ColumnType::Text)])
            .unwrap();
        store.save_row("users", b"1", &["1".into(), "Alice".into()]).unwrap();

        // Directly corrupt the underlying data bytes (bit-rot simulation)
        // without touching the recorded checksum -- exactly what scan_table
        // is meant to catch.
        let key = PersistentStore::data_key("users", b"1");
        let mut corrupted_json = serde_json::to_vec(&vec!["1".to_string(), "Alice".to_string()]).unwrap();
        corrupted_json[0] ^= 0xFF;
        store.data.insert(&key, corrupted_json).unwrap();

        let result = store.scan_table("users");
        assert!(matches!(result, Err(StorageError::ChecksumMismatch { .. })));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scrub_finds_corrupted_rows_without_stopping_at_the_first_one() {
        let dir = std::env::temp_dir().join(format!("aruaru-fjall-scrub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let store = PersistentStore::open(&dir).unwrap();
        store.save_schema("t", &[("id".to_string(), ColumnType::BigInt)]).unwrap();
        store.save_row("t", b"1", &["1".into()]).unwrap();
        store.save_row("t", b"2", &["2".into()]).unwrap();
        store.save_row("t", b"3", &["3".into()]).unwrap();

        // Corrupt rows 1 and 3, leave row 2 untouched.
        for pk in [b"1".as_slice(), b"3".as_slice()] {
            let key = PersistentStore::data_key("t", pk);
            let mut bytes = serde_json::to_vec(&vec![String::from_utf8_lossy(pk).into_owned()]).unwrap();
            bytes[0] ^= 0xFF;
            store.data.insert(&key, bytes).unwrap();
        }

        let corrupted = store.scrub().unwrap();
        assert_eq!(corrupted.len(), 2, "scrub should find both corrupted rows, not stop at the first");
        let corrupted_pks: Vec<Vec<u8>> = corrupted.into_iter().map(|(_, pk)| pk).collect();
        assert!(corrupted_pks.contains(&b"1".to_vec()));
        assert!(corrupted_pks.contains(&b"3".to_vec()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_table_skips_verification_when_no_checksum_recorded() {
        // Backward compatibility: a row written directly to the `data`
        // partition without a corresponding checksum entry (simulating
        // data written before this checksum layer existed) must not be
        // treated as corrupted.
        let dir = std::env::temp_dir().join(format!("aruaru-fjall-nochecksum-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let store = PersistentStore::open(&dir).unwrap();
        store.save_schema("t", &[("id".to_string(), ColumnType::BigInt)]).unwrap();

        // Write directly to the data partition, bypassing save_row (and
        // therefore never writing a checksum entry).
        let key = PersistentStore::data_key("t", b"1");
        let json = serde_json::to_vec(&vec!["1".to_string()]).unwrap();
        store.data.insert(&key, json).unwrap();

        let rows = store.scan_table("t").unwrap();
        assert_eq!(rows.len(), 1, "row without a checksum entry should still be readable");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
