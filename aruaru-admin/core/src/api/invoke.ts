/**
 * aruaru-DB Admin — GraphQL クライアント（REST 廃止・GraphQL 一本化）
 *
 * 問い合わせ(Query)は aruaru-server /graphql へ、管理操作(バックアップ・移行・クラスタ・統合など)は
 * Connect(`POST /open_runo.aruaru.admin.v1.Admin/<Method>`)へ送る(2026-11-09、REST全廃方針。
 * 公開 /graphql から管理 Mutation は外れた)。
 * 将来 Hive Gateway を差し込む場合は gqlEndpoint() の URL を切り替えるだけ。
 */

import { adminToken, gqlEndpoint } from "./config";

// ── 低レベル GQL 実行 ─────────────────────────────────────────

export interface GqlResult<T = any> {
  data?: T;
  errors?: { message: string }[];
}

/** 管理操作(Connect の単純呼び出し)。`POST {base}/open_runo.aruaru.admin.v1.Admin/<Method>`。 */
export async function connect<T = any>(method: string, body: Record<string, unknown> = {}): Promise<T> {
  const base = gqlEndpoint().replace(/\/graphql$/, "");
  const headers: Record<string, string> = { "Content-Type": "application/json", Accept: "application/json" };
  const t = adminToken();
  if (t) headers["x-admin-token"] = t;
  const res = await fetch(`${base}/open_runo.aruaru.admin.v1.Admin/${method}`, {
    method: "POST",
    headers,
    body: JSON.stringify(body),
  });
  const v: any = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(`${v.code ?? res.status}: ${v.message ?? "request failed"}`);
  return v as T;
}

export async function gql<T = any>(
  query: string,
  variables?: Record<string, unknown>
): Promise<GqlResult<T>> {
  const res = await fetch(gqlEndpoint(), {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "application/json", ...(adminToken() ? { "x-admin-token": adminToken()! } : {}) },
    body: JSON.stringify({ query, variables: variables ?? null }),
  });
  if (!res.ok) throw new Error(`GraphQL HTTP ${res.status}`);
  return res.json();
}

/** data の最初のフィールドを返す。エラーがあれば throw。 */
async function gqlFirst<T>(query: string, vars?: Record<string, unknown>): Promise<T> {
  const r = await gql<any>(query, vars);
  if (r.errors?.length) throw new Error(r.errors[0].message);
  const keys = Object.keys(r.data ?? {});
  if (!keys.length) throw new Error("empty GraphQL response");
  return r.data[keys[0]] as T;
}

type Args = Record<string, any>;

// ── Tauri 互換 invoke ─────────────────────────────────────────

export async function invoke<T = any>(cmd: string, args: Args = {}): Promise<T> {
  switch (cmd) {
    // ── ヘルスチェック ──────────────────────────────────────
    case "ping_server": {
      try {
        await gql("query { currentBranch }");
        return true as unknown as T;
      } catch {
        return false as unknown as T;
      }
    }

    // ── VCS ────────────────────────────────────────────────
    case "get_commit_log": {
      const limit = args.limit ?? 20;
      return gqlFirst<T>(
        `query($limit: Int!) { log(limit: $limit) { id shortId author message timestamp rootHash } }`,
        { limit }
      );
    }
    case "list_branches":
      return gqlFirst<T>(`query { branches { name headCommitId isCurrent } }`);

    case "graphql_query": {
      const r = await gql(args.query, args.variables);
      return r as unknown as T;
    }

    case "exec_sql":
      return gqlFirst<T>(
        `mutation($sql: String!, $autoCommit: Boolean, $msg: String) {
           execSql(sql: $sql, autoCommit: $autoCommit, commitMessage: $msg) {
             success commitId message
           }
         }`,
        { sql: args.sql, autoCommit: args.autoCommit ?? false, msg: args.message ?? null }
      );

    case "create_branch":
      return gqlFirst<T>(
        `mutation($name: String!) { createBranch(name: $name) { success message } }`,
        { name: args.name }
      );
    case "checkout_branch":
      return gqlFirst<T>(
        `mutation($branch: String!) { checkout(branch: $branch) { success message } }`,
        { branch: args.branch }
      );
    case "merge_branch":
      return gqlFirst<T>(
        `mutation($from: String!) { merge(fromBranch: $from) { success commitId message } }`,
        { from: args.from }
      );

    // ── レジストリ ───────────────────────────────────────────
    case "list_registry":
      return gqlFirst<T>(
        `query { registry { id name category wire status rank score updatedAt } }`
      );
    case "registry_summary":
      return gqlFirst<T>(
        `query { registrySummary { total connectable ga beta pgCompatible planned } }`
      );
    case "registry_crawl":
      return connect<T>("CrawlRegistry");
    case "registry_test":
      return connect<T>("TestRegistryConnection", { id: args.id, uri: args.uri });
    // ── バックアップ ─────────────────────────────────────────
    case "list_backups":
      return gqlFirst<T>(
        `query { backups { id createdAt branch commitId kind sizeMb path status } }`
      );
    case "create_backup":
      return connect<T>("CreateBackup", { config: { branch: args.branch ?? null, kind: args.kind ?? "full" } });
    case "restore_backup":
      return connect<T>("RestoreBackup", {
        input: { backupId: args.backupId ?? args.backup_id, targetBranch: args.targetBranch ?? null },
      });
    case "set_backup_schedule":
      return connect<T>("SetBackupSchedule", {
        input: { enabled: args.enabled, cron: args.cron, kind: args.kind ?? "full" },
      });
    // ── クラスタ ─────────────────────────────────────────────
    case "get_cluster_status":
      return gqlFirst<T>(
        `query {
           clusterStatus {
             stats { totalNodes healthyNodes totalRanges totalRows tableCount replicationFactor underReplicated }
             nodes { nodeId addr role alive commitIndex appliedIndex ranges diskUsedGb }
             ranges { rangeId startKey endKey leaderNode replicas sizeMb }
           }
         }`
      );
    case "add_cluster_node":
      return connect<T>("ClusterNodeOp", {
        input: { action: "add", nodeId: args.nodeId ?? args.node_id, addr: args.addr },
      });
    case "decommission_node":
      return connect<T>("ClusterNodeOp", {
        input: { action: "remove", nodeId: args.nodeId ?? args.node_id, addr: "" },
      });
    case "rebalance_cluster":
      return connect<T>("RebalanceCluster");
    case "cluster_propose":
      return connect<T>("ClusterPropose", { sql: args.sql });
    // ── 並列実行 ─────────────────────────────────────────────
    case "get_parallel_config":
      return gqlFirst<T>(
        `query { parallelConfig { enabled maxWorkers chunkSize strategy } }`
      );
    case "set_parallel_config":
      // 並列実行の設定は宣言的設定(aruaru.yaml の query.parallel、ホットリロード)で行う。
      // 旧 GraphQL `setParallelConfig` は2026-08-29に撤廃済み(この呼び出しは以前から失敗していた)。
      throw new Error("並列実行の設定は aruaru.yaml の query.parallel で行います(画面からは変更できません)");
    case "explain_distributed":
      return connect<T>("ExplainDistributed", { sql: args.sql });
    case "list_parallel_jobs":
      return gqlFirst<T>(
        `query { parallelJobs { jobId sql status workers elapsedMs rowsProcessed startedAt } }`
      );

    // ── フェデレーション (分散DB統合) ─────────────────────────
    case "list_federated_sources":
      return gqlFirst<T>(
        `query { federatedSources { name kind uri status tables } }`
      );
    case "register_federated_source":
      return connect<T>("RegisterFederatedSource", {
        input: { name: args.name, kind: args.kind, uri: args.uri },
      });
    case "test_federated_source":
      return connect<T>("TestSourceConnection", { source: args.kind, uri: args.uri });
    case "drop_federated_source":
      return connect<T>("DropFederatedSource", { name: args.name });
    case "federated_query":
      return connect<T>("FederatedQuery", { sql: args.sql });
    // ── マイグレーション ──────────────────────────────────────
    case "test_source_connection":
      return connect<T>("TestSourceConnection", { source: args.source, uri: args.uri });
    case "preview_source_schema":
      return connect<T>("PreviewSource", { source: args.source, uri: args.uri });
    case "run_migration":
      return connect<T>("RunMigration", {
        input: {
          source: args.source,
          sourceUri: args.source_uri ?? args.sourceUri,
          commitMessage: args.commit_message ?? args.commitMessage ?? null,
          includeTables: args.include_tables ?? args.includeTables ?? null,
        },
      });
    case "migrate_instance":
      return connect<T>("RunMigration", {
        input: {
          source: "aruaru",
          sourceUri: args.targetUri ?? args.target_uri,
          commitMessage: "Instance migration",
          includeTables: null,
        },
      });
    default:
      throw new Error(`unknown command: ${cmd}`);
  }
}
