//! Git-on-SQL バージョン管理レイヤー
//!
//! ## コミット ID
//! SHA-256(parent_id || root_hash || author || message || timestamp)
//! 40 hex 文字で表現（20 bytes）
//!
//! ## ブランチ
//! 名前付きポインタ。`redb` のメタデータストアに永続化。
//!
//! ## Prolly Tree
//! コンテンツアドレッサブルなチャンク境界を持つ B-tree 変種。
//! Dolt の設計思想を Pure Rust で再実装。
//! diff が O(変更量) で計算できる点が通常の B-tree と異なる。

pub mod branch;
pub mod commit;
pub mod diff;
pub mod prolly;

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

pub use branch::Branch;
pub use commit::{Commit, CommitId};
pub use diff::{Diff, DiffRow};

/// バージョン管理エラー
#[derive(Debug, thiserror::Error)]
pub enum VersionError {
    #[error("branch not found: {0}")]
    BranchNotFound(String),
    #[error("commit not found: {0}")]
    CommitNotFound(CommitId),
    #[error("merge conflict: {0} rows conflicted")]
    MergeConflict(usize),
    #[error("cannot fast-forward: branches have diverged")]
    CannotFastForward,
    #[error("storage error: {0}")]
    Storage(String),
}

pub type Result<T> = std::result::Result<T, VersionError>;

/// バージョンコントローラー: ブランチ・コミット・diff を管理
pub struct VersionController {
    /// ブランチ名 → 最新コミット ID のマップ
    branches: Arc<RwLock<HashMap<String, CommitId>>>,
    /// コミット ID → Commit のストア (メモリキャッシュ; 永続化は redb)
    commits: Arc<RwLock<HashMap<CommitId, Commit>>>,
    /// 現在のブランチ
    current_branch: Arc<RwLock<String>>,
}

impl VersionController {
    /// 新規初期化（main ブランチを作成）
    pub fn new() -> Self {
        let mut branches = HashMap::new();
        let genesis = Commit::genesis();
        let genesis_id = genesis.id.clone();
        let mut commits = HashMap::new();
        commits.insert(genesis_id.clone(), genesis);
        branches.insert("main".to_string(), genesis_id);

        Self {
            branches: Arc::new(RwLock::new(branches)),
            commits: Arc::new(RwLock::new(commits)),
            current_branch: Arc::new(RwLock::new("main".to_string())),
        }
    }

    /// 現在のブランチ名を取得
    pub fn current_branch(&self) -> String {
        self.current_branch.read().clone()
    }

    /// 現在のブランチの HEAD コミット
    pub fn head(&self) -> Option<Commit> {
        let branch = self.current_branch.read().clone();
        let branches = self.branches.read();
        let commit_id = branches.get(&branch)?;
        let commits = self.commits.read();
        commits.get(commit_id).cloned()
    }

    /// ブランチを作成
    pub fn create_branch(&self, name: &str) -> Result<()> {
        let head_id = {
            let branch = self.current_branch.read().clone();
            let branches = self.branches.read();
            branches
                .get(&branch)
                .cloned()
                .ok_or_else(|| VersionError::BranchNotFound(branch))?
        };
        let mut branches = self.branches.write();
        branches.insert(name.to_string(), head_id);
        tracing::info!(branch = name, "Branch created");
        Ok(())
    }

    /// ブランチを切り替え
    pub fn checkout(&self, name: &str) -> Result<()> {
        let branches = self.branches.read();
        if !branches.contains_key(name) {
            return Err(VersionError::BranchNotFound(name.to_string()));
        }
        drop(branches);
        *self.current_branch.write() = name.to_string();
        tracing::info!(branch = name, "Checked out branch");
        Ok(())
    }

    /// コミットを作成
    pub fn commit(&self, author: &str, message: &str, root_hash: [u8; 32]) -> Result<CommitId> {
        let parent = self.head().map(|c| c.id.clone());

        let commit = Commit::new(parent.clone(), root_hash, author, message);
        let commit_id = commit.id.clone();

        let mut commits = self.commits.write();
        commits.insert(commit_id.clone(), commit);

        let branch = self.current_branch.read().clone();
        let mut branches = self.branches.write();
        branches.insert(branch.clone(), commit_id.clone());

        tracing::info!(
            branch = %branch,
            commit = %commit_id,
            author = author,
            message = message,
            "Committed"
        );

        Ok(commit_id)
    }

    /// コミットログ (HEAD から祖先方向)
    pub fn log(&self, limit: usize) -> Vec<Commit> {
        let mut result = Vec::new();
        let mut current = self.head();
        while let Some(commit) = current {
            current = commit.parent.as_ref().and_then(|pid| {
                self.commits.read().get(pid).cloned()
            });
            result.push(commit);
            if result.len() >= limit {
                break;
            }
        }
        result
    }

    /// ブランチ一覧
    pub fn list_branches(&self) -> Vec<Branch> {
        let branches = self.branches.read();
        let current = self.current_branch.read().clone();
        branches
            .iter()
            .map(|(name, commit_id)| Branch {
                name: name.clone(),
                head: commit_id.clone(),
                is_current: name == &current,
            })
            .collect()
    }

    /// Fast-forward マージ
    pub fn fast_forward_merge(&self, from_branch: &str) -> Result<CommitId> {
        let branches = self.branches.read();
        let from_id = branches
            .get(from_branch)
            .cloned()
            .ok_or_else(|| VersionError::BranchNotFound(from_branch.to_string()))?;
        drop(branches);

        let current_branch = self.current_branch.read().clone();
        let mut branches = self.branches.write();
        branches.insert(current_branch.clone(), from_id.clone());

        tracing::info!(
            into = %current_branch,
            from = %from_branch,
            "Fast-forward merge"
        );
        Ok(from_id)
    }

    /// ブランチ名 → そのブランチ HEAD コミットの root_hash を取得
    fn branch_root_hash(&self, branch: &str) -> Option<[u8; 32]> {
        let branches = self.branches.read();
        let commit_id = branches.get(branch)?;
        let commits = self.commits.read();
        commits.get(commit_id).map(|c| c.root_hash)
    }

    /// 2 ブランチ間の diff を Prolly Tree の構造共有スキップで計算。
    /// `store` は両ブランチのツリーが格納された共有 NodeStore。
    pub fn diff_branches(
        &self,
        store: &crate::version::prolly::NodeStore,
        from_branch: &str,
        to_branch: &str,
    ) -> Result<Diff> {
        let from_root = self
            .branch_root_hash(from_branch)
            .ok_or_else(|| VersionError::BranchNotFound(from_branch.to_string()))?;
        let to_root = self
            .branch_root_hash(to_branch)
            .ok_or_else(|| VersionError::BranchNotFound(to_branch.to_string()))?;

        // ゼロハッシュ (genesis) は「空ツリー」として None 扱い
        let from = if from_root == [0u8; 32] { None } else { Some(from_root) };
        let to = if to_root == [0u8; 32] { None } else { Some(to_root) };

        let prolly_diffs = crate::version::prolly::diff_trees(store, from, to);

        // ProllyDiff → DiffRow に変換
        use crate::version::prolly::ProllyDiff;
        let rows = prolly_diffs
            .into_iter()
            .map(|d| match d {
                ProllyDiff::Added(k, v) => DiffRow {
                    table_id: 0,
                    pk: k.into(),
                    kind: diff::DiffKind::Added,
                    before: None,
                    after: Some(v.into()),
                },
                ProllyDiff::Removed(k, v) => DiffRow {
                    table_id: 0,
                    pk: k.into(),
                    kind: diff::DiffKind::Removed,
                    before: Some(v.into()),
                    after: None,
                },
                ProllyDiff::Modified(k, before, after) => DiffRow {
                    table_id: 0,
                    pk: k.into(),
                    kind: diff::DiffKind::Modified,
                    before: Some(before.into()),
                    after: Some(after.into()),
                },
            })
            .collect();

        Ok(Diff {
            from_commit: from_branch.to_string(),
            to_commit: to_branch.to_string(),
            rows,
        })
    }
}

impl Default for VersionController {
    fn default() -> Self {
        Self::new()
    }
}
