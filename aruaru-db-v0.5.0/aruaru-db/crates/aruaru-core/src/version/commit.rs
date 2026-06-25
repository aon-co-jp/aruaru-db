//! コミット型: Git コミットと等価な不変オブジェクト

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// コミット ID: SHA-256 の 32 bytes を hex 64 文字で表現
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommitId(String);

impl CommitId {
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(hex::encode(bytes))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 短縮表示 (先頭 12 文字)
    pub fn short(&self) -> &str {
        &self.0[..12]
    }
}

impl fmt::Display for CommitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &self.0[..12])  // 短縮形で表示
    }
}

/// コミットオブジェクト
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    /// このコミットの ID
    pub id: CommitId,
    /// 親コミット ID (genesis は None)
    pub parent: Option<CommitId>,
    /// Prolly Tree のルートハッシュ (= データ全体の指紋)
    pub root_hash: [u8; 32],
    /// 作者
    pub author: String,
    /// コミットメッセージ
    pub message: String,
    /// Unix nanoseconds
    pub timestamp: i64,
    /// スキーマバージョン
    pub schema_version: u32,
}

impl Commit {
    /// 通常のコミットを作成
    pub fn new(
        parent: Option<CommitId>,
        root_hash: [u8; 32],
        author: &str,
        message: &str,
    ) -> Self {
        let timestamp = Utc::now().timestamp_nanos_opt().unwrap_or(0);

        let id = Self::compute_id(parent.as_ref(), &root_hash, author, message, timestamp);

        Self {
            id,
            parent,
            root_hash,
            author: author.to_string(),
            message: message.to_string(),
            timestamp,
            schema_version: 1,
        }
    }

    /// ジェネシスコミット (初期空コミット)
    pub fn genesis() -> Self {
        Self::new(None, [0u8; 32], "system", "Initial commit")
    }

    /// コミット ID を計算
    fn compute_id(
        parent: Option<&CommitId>,
        root_hash: &[u8; 32],
        author: &str,
        message: &str,
        timestamp: i64,
    ) -> CommitId {
        let mut hasher = Sha256::new();
        if let Some(p) = parent {
            hasher.update(p.as_str().as_bytes());
        }
        hasher.update(root_hash);
        hasher.update(author.as_bytes());
        hasher.update(message.as_bytes());
        hasher.update(timestamp.to_le_bytes());
        let bytes: [u8; 32] = hasher.finalize().into();
        CommitId::from_bytes(&bytes)
    }

    /// タイムスタンプを人間が読める形式で返す
    pub fn timestamp_rfc3339(&self) -> String {
        use chrono::DateTime;
        let dt = DateTime::from_timestamp_nanos(self.timestamp);
        dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
    }
}
