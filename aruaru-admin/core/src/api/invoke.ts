/**
 * aruaru-DB Admin — GraphQL クライアント（REST 廃止・GraphQL 一本化）
 *
 * すべての操作を aruaru-server /graphql (async-graphql) へ GraphQL で送る。
 * 将来 Hive Gateway を差し込む場合は gqlEndpoint() の URL を切り替えるだけ。
 */

import { gqlEndpoint } from "./config";

// ── 低レベル GQL 実行 ─────────────────────────────────────────

export interface GqlResult<T = any> {
  data?: T;
  errors?: { message: string }[];
}

export async function gql<T = any>(
  query: string,
  variables?: Record<string, unknown>
): Promise<GqlResult<T>> {
  const res = await fetch(gqlEndpoint(), {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
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
      return gqlFirst<T>(
        `mutation { crawlRegistry { ok updated message } }`
      );
    case "registry_test":
      return gqlFirst<T>(
        `mutation($id: String!, $uri: String!) {
           testRegistryConnection(id: $id, uri: $uri) { ok message serverVersion }
         }`,
        { id: args.id, uri: args.uri }
      );

    // ── バックアップ ─────────────────────────────────────────
    case "list_backups":
      return gqlFirst<T>(
        `query { backups { id createdAt branch commitId kind sizeMb path status } }`
      );
    case "create_backup":
      return gqlFirst<T>(
        `mutation($branch: String, $kind: String) {
           createBackup(config: { branch: $branch, kind: $kind }) {
             id createdAt branch kind status
           }
         }`,
        { branch: args.branch ?? null, kind: args.kind ?? "full" }
      );
    case "restore_backup":
      return gqlFirst<T>(
        `mutation($id: String!, $target: String) {
           restoreBackup(input: { backupId: $id, targetBranch: $target }) {
             success message
           }
         }`,
        { id: args.backupId ?? args.backup_id, target: args.targetBranch ?? null }
      );
    case "set_backup_schedule":
      return gqlFirst<T>(
        `mutation($enabled: Boolean!, $cron: String!, $kind: String!) {
           setBackupSchedule(input: { enabled: $enabled, cron: $cron, kind: $kind }) {
             enabled cron kind nextRun
           }
         }`,
        { enabled: args.enabled, cron: args.cron, kind: args.kind ?? "full" }
      );

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
      return gqlFirst<T>(
        `mutation($nodeId: Int!, $addr: String!) {
           clusterNodeOp(input: { action: "add", nodeId: $nodeId, addr: $addr }) {
             success message
           }
         }`,
        { nodeId: args.nodeId ?? args.node_id, addr: args.addr }
      );
    case "decommission_node":
      return gqlFirst<T>(
        `mutation($nodeId: Int!) {
           clusterNodeOp(input: { action: "remove", nodeId: $nodeId, addr: "" }) {
             success message
           }
         }`,
        { nodeId: args.nodeId ?? args.node_id }
      );
    case "rebalance_cluster":
      return gqlFirst<T>(
        `mutation { rebalanceCluster { success message } }`
      );
    case "cluster_propose":
      return gqlFirst<T>(
        `mutation($sql: String!) { clusterPropose(sql: $sql) { success message } }`,
        { sql: args.sql }
      );

    // ── 並列実行 ─────────────────────────────────────────────
    case "get_parallel_config":
      return gqlFirst<T>(
        `query { parallelConfig { enabled maxWorkers chunkSize strategy } }`
      );
    case "set_parallel_config":
      return gqlFirst<T>(
        `mutation($enabled: Boolean!, $workers: Int!, $chunk: Int!, $strategy: String!) {
           setParallelConfig(config: {
             enabled: $enabled, maxWorkers: $workers, chunkSize: $chunk, strategy: $strategy
           }) { enabled maxWorkers chunkSize strategy }
         }`,
        {
          enabled: args.config?.enabled ?? args.enabled ?? false,
          workers: args.config?.max_workers ?? args.max_workers ?? 4,
          chunk:   args.config?.chunk_size ?? args.chunk_size ?? 10000,
          strategy: args.config?.strategy ?? args.strategy ?? "hash",
        }
      );
    case "explain_distributed":
      return gqlFirst<T>(
        `mutation($sql: String!) {
           explainDistributed(sql: $sql) { step node range operation estimatedRows }
         }`,
        { sql: args.sql }
      );
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
      return gqlFirst<T>(
        `mutation($name: String!, $kind: String!, $uri: String!) {
           registerFederatedSource(input: { name: $name, kind: $kind, uri: $uri }) {
             name kind uri status tables
           }
         }`,
        { name: args.name, kind: args.kind, uri: args.uri }
      );
    case "test_federated_source":
      return gqlFirst<T>(
        `mutation($kind: String!, $uri: String!) {
           testSourceConnection(source: $kind, uri: $uri) { ok message serverVersion }
         }`,
        { kind: args.kind, uri: args.uri }
      );
    case "drop_federated_source":
      return gqlFirst<T>(
        `mutation($name: String!) { dropFederatedSource(name: $name) { success message } }`,
        { name: args.name }
      );
    case "federated_query":
      return gqlFirst<T>(
        `mutation($sql: String!) {
           federatedQuery(sql: $sql) { columns rows commandTag }
         }`,
        { sql: args.sql }
      );

    // ── マイグレーション ──────────────────────────────────────
    case "test_source_connection":
      return gqlFirst<T>(
        `mutation($source: String!, $uri: String!) {
           testSourceConnection(source: $source, uri: $uri) { ok message serverVersion }
         }`,
        { source: args.source, uri: args.uri }
      );
    case "preview_source_schema":
      return gqlFirst<T>(
        `query($source: String!, $uri: String!) {
           previewSource(source: $source, uri: $uri) { schema name estimatedRows }
         }`,
        { source: args.source, uri: args.uri }
      );
    case "run_migration":
      return gqlFirst<T>(
        `mutation($source: String!, $uri: String!, $msg: String, $tables: [String!]) {
           runMigration(input: {
             source: $source, sourceUri: $uri, commitMessage: $msg, includeTables: $tables
           }) {
             success wire totalRows commitId message
             tables { table rows error }
           }
         }`,
        {
          source: args.source,
          uri: args.source_uri ?? args.sourceUri,
          msg: args.commit_message ?? args.commitMessage ?? null,
          tables: args.include_tables ?? args.includeTables ?? null,
        }
      );
    case "migrate_instance":
      return gqlFirst<T>(
        `mutation($uri: String!, $history: Boolean!) {
           runMigration(input: {
             source: "aruaru", sourceUri: $uri,
             commitMessage: "Instance migration",
             includeTables: null
           }) { success totalRows commitId message }
         }`,
        { uri: args.targetUri ?? args.target_uri, history: args.includeHistory ?? true }
      );

    default:
      throw new Error(`unknown command: ${cmd}`);
  }
}
