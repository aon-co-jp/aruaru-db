//! diff: 2 つのコミット間の行レベル差分

use bytes::Bytes;
use serde::{Deserialize, Serialize};

/// 差分の種別
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffKind {
    Added,
    Removed,
    Modified,
}

/// 差分の一行
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffRow {
    pub table_id: u32,
    pub pk: Bytes,
    pub kind: DiffKind,
    /// 変更前の値 (Added の場合 None)
    pub before: Option<Bytes>,
    /// 変更後の値 (Removed の場合 None)
    pub after: Option<Bytes>,
}

/// 2 コミット間の Diff 全体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diff {
    pub from_commit: String,
    pub to_commit: String,
    pub rows: Vec<DiffRow>,
}

impl Diff {
    pub fn added_count(&self) -> usize {
        self.rows.iter().filter(|r| r.kind == DiffKind::Added).count()
    }
    pub fn removed_count(&self) -> usize {
        self.rows.iter().filter(|r| r.kind == DiffKind::Removed).count()
    }
    pub fn modified_count(&self) -> usize {
        self.rows.iter().filter(|r| r.kind == DiffKind::Modified).count()
    }
}
