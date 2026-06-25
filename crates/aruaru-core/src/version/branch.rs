//! ブランチ型

use serde::{Deserialize, Serialize};
use super::CommitId;

/// ブランチ: 名前付き可変ポインタ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    pub head: CommitId,
    pub is_current: bool,
}
