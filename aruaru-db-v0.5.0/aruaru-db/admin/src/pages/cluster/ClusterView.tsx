// Tauri Admin: 分散DB管理・クラスタビューページ
import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface NodeStatus {
  node_id: number;
  addr: string;
  role: "Leader" | "Follower" | "Candidate" | "Offline";
  term: number;
  commit_index: number;
  applied_index: number;
  ranges: number;
  disk_used_gb: number;
  cpu_pct: number;
  last_heartbeat_ms: number;
}

interface RangeInfo {
  range_id: number;
  start_key: string;
  end_key: string;
  leader_node: number;
  replicas: number[];
  size_mb: number;
  rows: number;
}

interface ClusterStats {
  total_nodes: number;
  healthy_nodes: number;
  total_ranges: number;
  total_rows: number;
  total_disk_gb: number;
  raft_term: number;
  replication_factor: number;
}

const ROLE_COLOR = {
  Leader:    "text-yellow-400 bg-yellow-400/10",
  Follower:  "text-blue-400 bg-blue-400/10",
  Candidate: "text-orange-400 bg-orange-400/10",
  Offline:   "text-red-400 bg-red-400/10",
};

const ROLE_ICON = {
  Leader: "👑", Follower: "🔵", Candidate: "🟡", Offline: "🔴",
};

export default function ClusterView({ baseUrl }: { baseUrl: string }) {
  const [nodes, setNodes] = useState<NodeStatus[]>([]);
  const [ranges, setRanges] = useState<RangeInfo[]>([]);
  const [stats, setStats] = useState<ClusterStats | null>(null);
  const [tab, setTab] = useState<"nodes" | "ranges" | "rebalance" | "join">("nodes");
  const [newNodeAddr, setNewNodeAddr] = useState("");
  const [newNodeId, setNewNodeId] = useState("4");
  const [joinLoading, setJoinLoading] = useState(false);
  const [rebalancing, setRebalancing] = useState(false);

  // サーバから取得。未接続時はサンプル表示にフォールバック。
  const loadSample = () => {
    setStats({
      total_nodes: 3, healthy_nodes: 3, total_ranges: 12,
      total_rows: 2_450_000, total_disk_gb: 48.6, raft_term: 7, replication_factor: 3,
    });
    setNodes([
      { node_id: 1, addr: "10.0.0.1:5432", role: "Leader",   term: 7, commit_index: 1024, applied_index: 1024, ranges: 4, disk_used_gb: 16.2, cpu_pct: 12, last_heartbeat_ms: 50 },
      { node_id: 2, addr: "10.0.0.2:5432", role: "Follower", term: 7, commit_index: 1024, applied_index: 1023, ranges: 4, disk_used_gb: 15.8, cpu_pct: 8,  last_heartbeat_ms: 120 },
      { node_id: 3, addr: "10.0.0.3:5432", role: "Follower", term: 7, commit_index: 1024, applied_index: 1024, ranges: 4, disk_used_gb: 16.6, cpu_pct: 10, last_heartbeat_ms: 90 },
    ]);
    setRanges([
      { range_id: 1, start_key: "(min)", end_key: "m", leader_node: 1, replicas: [1,2,3], size_mb: 64, rows: 612000 },
      { range_id: 2, start_key: "m",    end_key: "z", leader_node: 2, replicas: [1,2,3], size_mb: 62, rows: 598000 },
      { range_id: 3, start_key: "z",    end_key: "(max)", leader_node: 3, replicas: [1,2,3], size_mb: 67, rows: 640000 },
    ]);
  };

  useEffect(() => {
    invoke<{ stats: ClusterStats; nodes: NodeStatus[]; ranges: RangeInfo[] }>("get_cluster_status", { baseUrl })
      .then((c) => { setStats(c.stats); setNodes(c.nodes); setRanges(c.ranges); })
      .catch(() => loadSample());
  }, [baseUrl]);

  const addNode = async () => {
    if (!newNodeAddr) return;
    setJoinLoading(true);
    try {
      await invoke("add_cluster_node", { baseUrl, nodeId: Number(newNodeId), addr: newNodeAddr });
      alert(`ノード ${newNodeId} (${newNodeAddr}) をクラスタに追加しました`);
    } catch {
      alert(`ノード追加要求: ${newNodeId} (${newNodeAddr})（サーバ未接続）`);
    }
    setJoinLoading(false);
    setNewNodeAddr("");
  };

  const rebalance = async () => {
    setRebalancing(true);
    try {
      await invoke("rebalance_cluster", { baseUrl });
      alert("Range のリバランスを開始しました");
    } catch {
      alert("リバランス要求（サーバ未接続）");
    }
    setRebalancing(false);
  };

  return (
    <div className="p-6 space-y-5">
      <h2 className="text-xl font-bold text-gray-100">🌐 クラスタ管理</h2>

      {/* クラスタ統計 */}
      {stats && (
        <div className="grid grid-cols-4 gap-3">
          {[
            ["ノード", `${stats.healthy_nodes} / ${stats.total_nodes}`, stats.healthy_nodes < stats.total_nodes ? "text-red-400" : "text-green-400"],
            ["Range", stats.total_ranges.toString(), "text-blue-400"],
            ["総行数", stats.total_rows.toLocaleString(), "text-orange-400"],
            ["Raft Term", `#${stats.raft_term}`, "text-purple-400"],
            ["総ディスク", `${stats.total_disk_gb.toFixed(1)} GB`, "text-gray-300"],
            ["レプリカ数", `${stats.replication_factor}`, "text-gray-300"],
          ].map(([label, val, cls]) => (
            <div key={label} className="bg-gray-900 rounded-lg p-3">
              <p className="text-xs text-gray-500">{label}</p>
              <p className={`text-lg font-bold mt-0.5 ${cls}`}>{val}</p>
            </div>
          ))}
        </div>
      )}

      {/* タブ */}
      <div className="flex gap-1 border-b border-gray-800">
        {([["nodes","ノード一覧"],["ranges","Range 分布"],["rebalance","リバランス"],["join","ノード追加"]] as const).map(([id, label]) => (
          <button key={id} onClick={() => setTab(id)}
            className={`px-4 py-2 text-sm ${tab === id ? "border-b-2 border-orange-500 text-orange-400" : "text-gray-500 hover:text-gray-300"}`}>
            {label}
          </button>
        ))}
      </div>

      {/* ノード一覧 */}
      {tab === "nodes" && (
        <div className="space-y-2">
          {nodes.map(n => (
            <div key={n.node_id} className="bg-gray-900 rounded-xl p-4">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-3">
                  <span className="text-2xl">{ROLE_ICON[n.role]}</span>
                  <div>
                    <div className="font-medium text-gray-200">Node {n.node_id}</div>
                    <div className="text-xs text-gray-500 font-mono">{n.addr}</div>
                  </div>
                  <span className={`text-xs px-2 py-0.5 rounded-full font-medium ${ROLE_COLOR[n.role]}`}>
                    {n.role}
                  </span>
                </div>
                <div className="text-right text-xs text-gray-500">
                  <div>HB: {n.last_heartbeat_ms}ms</div>
                  <div>Term: {n.term}</div>
                </div>
              </div>
              <div className="grid grid-cols-4 gap-3 text-xs">
                {[["Ranges",n.ranges],["Commit",n.commit_index],["Disk",`${n.disk_used_gb}GB`]].map(([k,v]) => (
                  <div key={k} className="bg-gray-800 rounded p-2">
                    <p className="text-gray-500">{k}</p>
                    <p className="text-gray-200 font-medium mt-0.5">{v}</p>
                  </div>
                ))}
                <div className="bg-gray-800 rounded p-2">
                  <p className="text-gray-500">CPU</p>
                  <div className="mt-1 bg-gray-700 rounded-full h-1.5">
                    <div className={`h-1.5 rounded-full ${n.cpu_pct > 80 ? "bg-red-500" : "bg-green-500"}`}
                      style={{ width: `${n.cpu_pct}%` }} />
                  </div>
                  <p className="text-gray-400 text-xs mt-0.5">{n.cpu_pct}%</p>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Range 分布 */}
      {tab === "ranges" && (
        <table className="w-full text-sm">
          <thead>
            <tr className="text-xs text-gray-500 border-b border-gray-800">
              <th className="text-left py-2">Range ID</th>
              <th className="text-left py-2">キー範囲</th>
              <th className="text-left py-2">Leader</th>
              <th className="text-left py-2">レプリカ</th>
              <th className="text-right py-2">サイズ</th>
              <th className="text-right py-2">行数</th>
            </tr>
          </thead>
          <tbody>
            {ranges.map(r => (
              <tr key={r.range_id} className="border-b border-gray-800/50 hover:bg-gray-800/30">
                <td className="py-2 text-orange-400 font-mono">#{r.range_id}</td>
                <td className="py-2 font-mono text-xs text-gray-400">{r.start_key} → {r.end_key}</td>
                <td className="py-2 text-yellow-400">Node {r.leader_node}</td>
                <td className="py-2 text-gray-500 text-xs">[{r.replicas.join(",")}]</td>
                <td className="py-2 text-right text-gray-400">{r.size_mb} MB</td>
                <td className="py-2 text-right text-gray-300">{r.rows.toLocaleString()}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {/* リバランス */}
      {tab === "rebalance" && (
        <div className="space-y-4">
          <p className="text-sm text-gray-400">
            Range を全ノードに均等に再分配します。大規模クラスタでは数分かかる場合があります。
          </p>
          <div className="bg-gray-900 rounded-xl p-4 space-y-2">
            <p className="text-sm font-medium text-gray-300">現在の分布状況</p>
            {nodes.map(n => (
              <div key={n.node_id} className="flex items-center gap-3">
                <span className="text-xs text-gray-500 w-16">Node {n.node_id}</span>
                <div className="flex-1 bg-gray-800 rounded-full h-2">
                  <div className="bg-blue-500 h-2 rounded-full"
                    style={{ width: `${(n.ranges / 12) * 100}%` }} />
                </div>
                <span className="text-xs text-gray-400">{n.ranges} ranges</span>
              </div>
            ))}
          </div>
          <button onClick={rebalance} disabled={rebalancing}
            className="px-6 py-2 bg-blue-600 hover:bg-blue-700 disabled:opacity-40 rounded text-white">
            {rebalancing ? "リバランス中..." : "リバランス実行 ⚖️"}
          </button>
        </div>
      )}

      {/* ノード追加 */}
      {tab === "join" && (
        <div className="space-y-4 max-w-md">
          <p className="text-sm text-gray-400">
            新しいノードをクラスタに参加させます。対象ノードで aruaru-server を起動済みである必要があります。
          </p>
          <div className="space-y-3">
            <div>
              <label className="text-xs text-gray-500 block mb-1">新ノード ID</label>
              <input value={newNodeId} onChange={e => setNewNodeId(e.target.value)}
                className="bg-gray-800 border border-gray-700 rounded px-3 py-1.5 text-sm text-gray-200 w-24" />
            </div>
            <div>
              <label className="text-xs text-gray-500 block mb-1">アドレス (host:port)</label>
              <input value={newNodeAddr} onChange={e => setNewNodeAddr(e.target.value)}
                placeholder="10.0.0.4:5432"
                className="bg-gray-800 border border-gray-700 rounded px-3 py-1.5 text-sm text-gray-200 font-mono w-full" />
            </div>
            <button onClick={addNode} disabled={!newNodeAddr || joinLoading}
              className="px-6 py-2 bg-green-600 hover:bg-green-700 disabled:opacity-40 rounded text-white">
              {joinLoading ? "追加中..." : "ノードを追加 ➕"}
            </button>
          </div>
          <div className="bg-gray-900 rounded-xl p-4 text-xs text-gray-500 font-mono space-y-1">
            <p className="text-gray-400 font-sans text-xs font-medium mb-2">新ノード側で実行するコマンド:</p>
            <p>aruaru-server \</p>
            <p className="ml-2">--data /var/lib/aruaru \</p>
            <p className="ml-2">--raft-id {newNodeId} \</p>
            <p className="ml-2">--peers {nodes.map(n => n.addr).join(",")}</p>
          </div>
        </div>
      )}
    </div>
  );
}
