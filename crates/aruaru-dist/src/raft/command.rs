//! Raft で複製する書き込みコマンド
//!
//! Leader が受理した書き込みを Command としてログに積み、各 Follower へ複製。
//! commit された Command を状態機械 (Applier) が QueryEngine へ適用する。

use serde::{Deserialize, Serialize};

/// 複製対象の書き込み操作
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// 書き込み SQL (INSERT/UPDATE/DELETE/CREATE TABLE/DROP TABLE)
    Exec(String),
    /// バージョンコミット (aruaru_commit)
    Commit(String),
    /// リーダー確立時のバリア (no-op)。空ログ期でも commit index を進められる。
    Noop,
}

impl Command {
    /// ログ payload へシリアライズ
    pub fn encode(&self) -> Vec<u8> {
        rust_json::to_vec_strict(self).unwrap_or_default()
    }
    /// payload からデコード
    pub fn decode(payload: &[u8]) -> Option<Command> {
        rust_json::from_slice_strict(payload).ok()
    }
}

/// Command 適用結果
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CommandResponse {
    pub ok: bool,
    pub message: String,
}

impl CommandResponse {
    pub fn ok() -> Self {
        Self { ok: true, message: String::new() }
    }
    pub fn err(msg: impl Into<String>) -> Self {
        Self { ok: false, message: msg.into() }
    }
}
