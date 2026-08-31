//! ephemeral SQL pod(CockroachDB Serverless方式のプロセスレベル使い捨て
//! 計算単位)の共有型・trait。
//!
//! 【2026-08-31 移設】元々`aruaru-server::ephemeral_pod`にあった型定義
//! (`EphemeralTable`/`EphemeralRequest`/`EphemeralResponse`)と
//! `snapshot_for_tenant`を、`aruaru-server`(REST/GraphQL両方の実装が
//! 依存できる)ではなく両者が依存できる本クレートへ移設した——
//! `aruaru-graphql::AdminCtx`から`ephemeral-query`をGraphQL化するには、
//! `aruaru-graphql`が参照できる場所にこれらの型・trait が無ければならず、
//! `aruaru-graphql`は`aruaru-server`のmodを参照できない(循環依存になる
//! ため)。`admin_shared.rs`/`keyring.rs`と同じ理由・同じパターン。
//!
//! **実際にOSプロセスを起動する処理(`current_exe()`+
//! `tokio::process::Command`)は`aruaru-server`側に残る**——これは
//! 「自分自身の実行ファイルを子プロセスとして起動する」という
//! `aruaru-server`バイナリ固有の処理であり、本クレート(ライブラリ)に
//! 持ち込むべきではない。代わりに`EphemeralRunner` traitを新設し、
//! `aruaru-server`がこれを実装した`ProcessEphemeralRunner`を
//! `Arc<dyn EphemeralRunner>`として`AdminCtx`へ注入する
//! (`ReplicatedWriter`と同じ「実装はサーバー側、trait定義は共有クレート」
//! という既存パターン)。

use aruaru_core::catalog::ColumnType;
use aruaru_query::QueryResponse;
use serde::{Deserialize, Serialize};

/// 子プロセスへ渡すテーブルスナップショット(列名はTEXT型として単純化)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EphemeralTable {
    pub name: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// 親→子(標準入力)リクエスト。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EphemeralRequest {
    pub tenant_id: String,
    pub tables: Vec<EphemeralTable>,
    pub sql: String,
}

/// 子→親(標準出力)レスポンス。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EphemeralResponse {
    pub ok: bool,
    pub result: Option<QueryResponse>,
    pub error: Option<String>,
}

/// 【親プロセス側、REST/GraphQL共通】現在のテーブル群のスナップショットを、
/// 指定テナントのephemeral podで実行するために使いやすい形へ変換する。
pub fn snapshot_for_tenant(
    engine: &aruaru_query::QueryEngine,
    table_names: &[String],
) -> Vec<EphemeralTable> {
    table_names
        .iter()
        .filter_map(|name| {
            let (cols, _pks, rows) = engine.snapshot_table(name)?;
            let columns: Vec<String> = cols.into_iter().map(|(n, _t): (String, ColumnType)| n).collect();
            Some(EphemeralTable {
                name: name.clone(),
                columns,
                rows,
            })
        })
        .collect()
}

/// ephemeral SQL podを実際に実行する抽象化。実装(実プロセス起動)は
/// `aruaru-server::ephemeral_pod::ProcessEphemeralRunner`が持つ——
/// このtraitはREST(`aruaru-server::admin`)とGraphQL
/// (`aruaru-graphql::admin_resolvers`)の両方から同じ実装を呼べるように
/// するための境界。
#[async_trait::async_trait]
pub trait EphemeralRunner: Send + Sync {
    async fn run(&self, request: &EphemeralRequest) -> anyhow::Result<EphemeralResponse>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// テスト専用のモック実装——実プロセスを起動せず、受け取った
    /// リクエストの`sql`をそのままエコーする。GraphQL resolver側の
    /// 配線(trait経由の呼び出し)を、実プロセスを起動せずに検証できる
    /// ようにするため。
    struct EchoRunner;

    #[async_trait::async_trait]
    impl EphemeralRunner for EchoRunner {
        async fn run(&self, request: &EphemeralRequest) -> anyhow::Result<EphemeralResponse> {
            Ok(EphemeralResponse {
                ok: true,
                result: None,
                error: Some(format!("echo: {} tables={}", request.sql, request.tables.len())),
            })
        }
    }

    #[tokio::test]
    async fn ephemeral_runner_trait_is_object_safe_and_callable_via_arc_dyn() {
        let runner: Arc<dyn EphemeralRunner> = Arc::new(EchoRunner);
        let req = EphemeralRequest {
            tenant_id: "t1".into(),
            tables: vec![EphemeralTable { name: "x".into(), columns: vec!["a".into()], rows: vec![] }],
            sql: "SELECT 1".into(),
        };
        let resp = runner.run(&req).await.unwrap();
        assert!(resp.ok);
        assert_eq!(resp.error.unwrap(), "echo: SELECT 1 tables=1");
    }
}
