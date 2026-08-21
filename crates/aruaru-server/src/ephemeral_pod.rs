//! 【2026-08-21新設】ephemeral SQL pod 化 — 第一歩
//!
//! CockroachDB Serverless はテナントごとに ephemeral な "SQL Pod"
//! (計算資源、Kubernetes pod単位でCPU/メモリ/帯域をcgroup制限)を
//! スケジューリングし、ストレージ層(KV+Raft)とは独立にスケールさせる
//! (`cockroachlabs.atlassian.net`の Cluster virtualization ドキュメント、
//! `cockroachdb/cockroach` issue #48119参照、CLAUDE.md HANDOFF
//! 2026-08-21参照)。
//!
//! この開発機(単一マシン)では Kubernetes 相当のオーケストレーション層は
//! 用意できないが、「テナントのクエリ処理を独立した OS プロセスとして
//! 起動し、処理完了後に終了させる」という**プロセスレベルの使い捨て
//! 計算単位**は実装可能であり、今回はそれを実装した。
//!
//! ## 設計
//! - 親プロセス (常駐 `aruaru-server`) が、対象テナントのテーブル
//!   スナップショットを JSON にシリアライズし、`tokio::process::Command`
//!   で **自分自身の実行ファイル** を `--ephemeral-worker` フラグ付きで
//!   子プロセスとして spawn する。
//! - 子プロセスは標準入力から JSON リクエストを読み、完全に独立した
//!   インメモリ `QueryEngine`(永続ストレージ・Raft・pgwire・GraphQL は
//!   一切起動しない)を新規構築してテーブルを再現し、SQL を1回だけ実行、
//!   結果を JSON で標準出力へ書いて **即座に終了する**。
//! - 親プロセスは子プロセスの標準出力(1回分)を読み取り、JSON をパース
//!   して呼び出し元へ返す。子プロセスは処理が終わるたびに必ず終了する
//!   ため、プロセスが「使い捨て」であることが実際のOSプロセス生成・
//!   終了で保証される。
//!
//! ## 正直な開示・スコープの限界
//! 1. **真のリソース制限(cgroup等)は行っていない** —
//!    子プロセスの CPU/メモリ/ネットワーク帯域を OS レベルで制限する
//!    仕組み(Linux cgroup、Windows Job Object)は今回実装していない。
//!    「独立したOSプロセスとして起動・終了する」という**プロセス分離
//!    そのもの**を実証する段階に留まる。
//! 2. **永続ストレージ(fjall)には触れない** — 子プロセスが親プロセスと
//!    同じ fjall データディレクトリを同時に開こうとするとファイル
//!    ロック競合(最悪はデータ破損)のリスクがあるため、意図的に
//!    テーブルスナップショットをJSON経由で受け渡す設計にした
//!    (子プロセスは完全にインメモリ、`ingest_table`のみ使用)。
//!    そのため、この ephemeral worker は「読み取り専用のテナント別
//!    計算オフロード」に限定される — 子プロセス内での書き込みは
//!    子プロセスのメモリ上でのみ完結し、親プロセスの永続状態には
//!    反映されない(この制約はドキュメント化し、書き込みを伴う実運用
//!    経路への配線は行っていない)。
//! 3. 複数物理マシンをまたぐ真のスケジューリング(Kubernetes pod 相当)
//!    はこの環境では検証不可能 — 単一マシン上の複数プロセスとしての
//!    検証に留まる。

use std::io::Write as _;
use std::process::Stdio;

use aruaru_core::catalog::ColumnType;
use aruaru_query::QueryResponse;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt as _;
use tokio::process::Command;

/// 子プロセスへ渡すテーブルスナップショット (列名は TEXT 型として単純化)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EphemeralTable {
    pub name: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// 親→子 (標準入力) リクエスト
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EphemeralRequest {
    pub tenant_id: String,
    pub tables: Vec<EphemeralTable>,
    pub sql: String,
}

/// 子→親 (標準出力) レスポンス
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EphemeralResponse {
    pub ok: bool,
    pub result: Option<QueryResponse>,
    pub error: Option<String>,
}

/// 【親プロセス側】現在のテーブル群のスナップショットを、指定テナントの
/// ephemeral pod で実行するために使いやすい形へ変換する。
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

/// 【親プロセス側】ephemeral SQL pod を実際に子プロセスとして起動し、
/// SQL を1回実行させて結果を受け取る。子プロセスは応答後に必ず終了する
/// (`tokio::process::Command` の `wait_with_output` が終了を待ち合わせる
/// ため、呼び出しが返った時点で子プロセスは既に終了している)。
pub async fn run_ephemeral_query(
    exe_path: &std::path::Path,
    request: &EphemeralRequest,
) -> anyhow::Result<EphemeralResponse> {
    let payload = serde_json::to_vec(request)?;

    let mut child = Command::new(exe_path)
        .arg("--ephemeral-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("ephemeral worker: failed to open stdin"))?;
        stdin.write_all(&payload).await?;
        stdin.shutdown().await?;
    }

    let output = child.wait_with_output().await?;
    if !output.status.success() {
        anyhow::bail!(
            "ephemeral worker exited with status {:?} (tenant={})",
            output.status.code(),
            request.tenant_id
        );
    }
    let resp: EphemeralResponse = serde_json::from_slice(&output.stdout)?;
    Ok(resp)
}

/// 【子プロセス側 (`--ephemeral-worker`)】標準入力から1件だけリクエストを
/// 読み取り、独立したインメモリ QueryEngine 上で SQL を実行して標準出力へ
/// 結果を書き、呼び出し元(`main.rs`)へ制御を返す。この関数が返った直後
/// プロセスは終了する想定 — 常駐処理は一切持たない。
pub fn run_worker_once() -> anyhow::Result<()> {
    use std::io::Read as _;
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let request: EphemeralRequest = serde_json::from_str(&input)?;

    // 完全に独立した、永続ストレージ・Raft・ネットワークを一切持たない
    // インメモリ QueryEngine を子プロセスの寿命の中だけで構築する。
    let engine = aruaru_query::QueryEngine::new();
    for t in &request.tables {
        engine.ingest_table(&t.name, t.columns.clone(), t.rows.clone());
    }

    // 【2026-08-21】この子プロセス自体が既にテナント単位で使い捨てられる
    // 独立したQueryEngineインスタンス(プロセス境界そのものが分離の単位)
    // であるため、`execute_as_tenant`のテーブル名前置(単一プロセス内で
    // 複数テナントの表を同居させるための仕組み、`admin.rs`の
    // `/admin/ephemeral-query`とは別の軸)は不要——ここでは
    // `ingest_table`で入れたテーブル名をそのまま`execute`する。
    let response = match engine.execute(&request.sql) {
        Ok(result) => EphemeralResponse { ok: true, result: Some(result), error: None },
        Err(e) => EphemeralResponse { ok: false, result: None, error: Some(e) },
    };

    let out = serde_json::to_vec(&response)?;
    std::io::stdout().write_all(&out)?;
    std::io::stdout().flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実際にこの実行ファイル自身を `--ephemeral-worker` フラグ付きで
    /// 子プロセスとして起動し、標準入力/標準出力越しに実際のJSON
    /// リクエスト/レスポンスをやり取りできることを検証する。
    /// `#[ignore]` — テストバイナリ自身は `--ephemeral-worker` を
    /// 解釈できない(統合テストは実際の `aruaru-server` バイナリに対して
    /// 行う、下記コメント参照)ため、CIのデフォルト実行では走らせない。
    #[test]
    #[ignore = "requires the built aruaru-server binary; see main.rs integration notes"]
    fn placeholder_for_binary_integration_test() {
        // 実際のプロセス間検証は `cargo build --release -p aruaru-server`
        // 後、生成された `aruaru-server(.exe)` を対象に手動/スクリプトで
        // 実施する(このクレート自体の cargo test バイナリではフラグ
        // ディスパッチができないため)。詳細はCLAUDE.mdのHANDOFF参照。
    }
}
