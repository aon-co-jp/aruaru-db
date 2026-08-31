//! `EngineApplier` — Raft commit を `aruaru_query::QueryEngine` へ適用する
//! 状態機械。
//!
//! 【2026-08-31 移設】元々`aruaru-server::cluster`にあったが、
//! `MultiRaftCluster<EngineApplier>`をGraphQL(`aruaru-graphql::
//! AdminCtx`)から共有するには、`aruaru-graphql`が具体的な`Applier`型を
//! 名指しできる必要がある(`aruaru-graphql`は`aruaru-server`のmodを
//! 参照できないため)。`EngineApplier`自体は`aruaru_query::QueryEngine`
//! にしか依存しない小さな構造体であり、`aruaru-dist`が既に`ephemeral.rs`
//! (2026-08-31新設)の都合で`aruaru-query`へ依存するようになったため、
//! ここへ移設することで新たな依存を増やさずに済む
//! (`admin_shared.rs`/`keyring.rs`/`ephemeral.rs`と同じ「共有クレートへ
//! 型を移設する」パターン)。`aruaru-server::cluster::EngineApplier`は
//! この型への再エクスポートとして維持し、既存コードへの影響を無くす。

use std::sync::Arc;

use aruaru_query::QueryEngine;

use crate::raft::{Applier, Command, CommandResponse};

/// Raft commit を QueryEngine へ適用する状態機械
pub struct EngineApplier {
    engine: Arc<QueryEngine>,
}

impl EngineApplier {
    pub fn new(engine: Arc<QueryEngine>) -> Self {
        Self { engine }
    }
}

impl Applier for EngineApplier {
    fn apply(&self, command: &Command) -> CommandResponse {
        match command {
            Command::Exec(sql) => match self.engine.execute(sql) {
                Ok(_) => CommandResponse::ok(),
                Err(e) => CommandResponse::err(e),
            },
            Command::Commit(msg) => {
                let safe = msg.replace('\'', "''");
                match self.engine.execute(&format!("SELECT aruaru_commit('{safe}')")) {
                    Ok(_) => CommandResponse::ok(),
                    Err(e) => CommandResponse::err(e),
                }
            }
            Command::Noop => CommandResponse::ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_and_commit_apply_to_the_shared_engine() {
        let engine = Arc::new(QueryEngine::new());
        let applier = EngineApplier::new(engine.clone());
        let resp = applier.apply(&Command::Exec("CREATE TABLE t (id INT PRIMARY KEY, v TEXT)".into()));
        assert!(resp.ok, "exec should succeed: {resp:?}");
        let resp = applier.apply(&Command::Commit("initial".into()));
        assert!(resp.ok, "commit should succeed: {resp:?}");
        assert!(engine.table_names().contains(&"t".to_string()));
    }
}
