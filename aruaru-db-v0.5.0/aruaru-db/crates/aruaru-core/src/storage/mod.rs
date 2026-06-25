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

/// ストレージエラー
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("fjall error: {0}")]
    Fjall(#[from] fjall::Error),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
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
}

impl PersistentStore {
    /// 指定パスにキースペースを開く (なければ作成)
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let keyspace = Config::new(path).open()?;
        let meta = keyspace.open_partition("__meta", PartitionCreateOptions::default())?;
        let data = keyspace.open_partition("__data", PartitionCreateOptions::default())?;
        Ok(Self {
            keyspace,
            meta,
            data,
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

    /// 1 行を保存 (pk と行の文字列配列)
    pub fn save_row(&self, table: &str, pk: &[u8], row: &[String]) -> Result<()> {
        let key = Self::data_key(table, pk);
        let json = serde_json::to_vec(row)?;
        self.data.insert(key, json)?;
        Ok(())
    }

    /// 1 行を削除
    pub fn delete_row(&self, table: &str, pk: &[u8]) -> Result<()> {
        self.data.remove(Self::data_key(table, pk))?;
        Ok(())
    }

    /// テーブルの全行を走査 (pk, 行)
    pub fn scan_table(&self, table: &str) -> Result<Vec<(Vec<u8>, Vec<String>)>> {
        let mut prefix = table.as_bytes().to_vec();
        prefix.push(0);
        let plen = prefix.len();

        let mut out = Vec::new();
        for kv in self.data.prefix(prefix.clone()) {
            let (k, v) = kv?;
            let pk = k[plen..].to_vec();
            let row: Vec<String> = serde_json::from_slice(&v)?;
            out.push((pk, row));
        }
        Ok(out)
    }

    /// テーブルを丸ごと削除 (スキーマ + 全行)
    pub fn drop_table(&self, table: &str) -> Result<()> {
        // スキーマ削除
        self.meta.remove(table.as_bytes())?;
        // データ行を prefix 走査して全削除
        let mut prefix = table.as_bytes().to_vec();
        prefix.push(0);
        let keys: Vec<Vec<u8>> = self
            .data
            .prefix(prefix)
            .filter_map(|kv| kv.ok().map(|(k, _)| k.to_vec()))
            .collect();
        for k in keys {
            self.data.remove(k)?;
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
}
