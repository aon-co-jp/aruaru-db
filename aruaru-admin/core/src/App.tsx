// aruaru-DB Admin GUI - React フロントエンド (Tauri 2 + React + TypeScript)
import { useState, useEffect } from "react";
import { invoke } from "./api/invoke";
import { serverBase, gqlEndpoint } from "./api/config";
import Dashboard from "./pages/Dashboard";
import CommitLog from "./pages/CommitLog";
import BranchView from "./pages/BranchView";
import QueryEditor from "./pages/QueryEditor";
import MigrationWizard from "./pages/MigrationWizard";
import BackupManager from "./pages/backup/BackupManager";
import ParallelView from "./pages/parallel/ParallelView";
import FederationView from "./pages/federation/FederationView";
import ClusterView from "./pages/cluster/ClusterView";
import DatabaseRegistry from "./pages/registry/DatabaseRegistry";

const SERVER_BASE = serverBase();
const SERVER_GQL = gqlEndpoint();

type Page =
  | "dashboard" | "commits" | "branches" | "query"
  | "migrate" | "backup" | "parallel" | "federation" | "cluster" | "registry";

interface ServerStatus { online: boolean; version: string; }

export default function App() {
  const [page, setPage] = useState<Page>("dashboard");
  const [serverStatus, setServerStatus] = useState<ServerStatus>({ online: false, version: "" });
  const [currentBranch, setCurrentBranch] = useState<string>("main");

  useEffect(() => {
    const checkServer = async () => {
      const online = await invoke<boolean>("ping_server", { url: SERVER_BASE }).catch(() => false);
      setServerStatus({ online, version: "0.5.0" });
    };
    checkServer();
    const interval = setInterval(checkServer, 5000);
    return () => clearInterval(interval);
  }, []);

  // ナビ: section でグループ化
  const navSections: { title: string; items: { id: Page; label: string; icon: string }[] }[] = [
    {
      title: "データ",
      items: [
        { id: "dashboard", label: "ダッシュボード", icon: "🏠" },
        { id: "commits",   label: "コミットログ",   icon: "📝" },
        { id: "branches",  label: "ブランチ",       icon: "🌿" },
        { id: "query",     label: "クエリ",         icon: "⚡" },
      ],
    },
    {
      title: "運用・分散",
      items: [
        { id: "migrate",    label: "お引越し",     icon: "🚚" },
        { id: "backup",     label: "バックアップ", icon: "💾" },
        { id: "registry",   label: "対応DB",       icon: "🗃️" },
        { id: "parallel",   label: "分散並列化",   icon: "⚙️" },
        { id: "federation", label: "分散DB統合",   icon: "🌐" },
        { id: "cluster",    label: "クラスタ",     icon: "🖧" },
      ],
    },
  ];

  return (
    <div className="flex h-screen bg-gray-950 text-gray-100 font-mono">
      {/* サイドバー */}
      <aside className="w-56 bg-gray-900 border-r border-gray-800 flex flex-col">
        <div className="p-4 border-b border-gray-800">
          <h1 className="text-lg font-bold text-orange-400">🦀 aruaru-DB</h1>
          <p className="text-xs text-gray-500 mt-1">Admin v0.5.0</p>
        </div>

        <div className="px-4 py-3 border-b border-gray-800">
          <div className="flex items-center gap-2">
            <div className={`w-2 h-2 rounded-full ${serverStatus.online ? "bg-green-400" : "bg-red-500"}`} />
            <span className="text-xs text-gray-400">{serverStatus.online ? "接続中" : "未接続"}</span>
          </div>
          <div className="mt-1 text-xs text-orange-300 flex items-center gap-1">
            <span>🌿</span><span>{currentBranch}</span>
          </div>
        </div>

        <nav className="flex-1 p-2 overflow-y-auto">
          {navSections.map((section) => (
            <div key={section.title} className="mb-3">
              <div className="px-3 py-1 text-[10px] uppercase tracking-wider text-gray-600">{section.title}</div>
              {section.items.map((item) => (
                <button key={item.id} onClick={() => setPage(item.id)}
                  className={`w-full text-left px-3 py-2 rounded-lg text-sm mb-1 transition-colors ${
                    page === item.id ? "bg-orange-500/20 text-orange-300" : "text-gray-400 hover:bg-gray-800 hover:text-gray-200"
                  }`}>
                  <span className="mr-2">{item.icon}</span>{item.label}
                </button>
              ))}
            </div>
          ))}
        </nav>

        <div className="p-3 border-t border-gray-800 text-xs text-gray-600">Apache-2.0 · OSS</div>
      </aside>

      {/* メインコンテンツ */}
      <main className="flex-1 overflow-auto">
        {page === "dashboard"  && <Dashboard serverUrl={SERVER_GQL} onBranchChange={setCurrentBranch} />}
        {page === "commits"    && <CommitLog serverUrl={SERVER_GQL} />}
        {page === "branches"   && <BranchView serverUrl={SERVER_GQL} onBranchChange={setCurrentBranch} />}
        {page === "query"      && <QueryEditor serverUrl={SERVER_GQL} />}
        {page === "migrate"    && <MigrationWizard baseUrl={SERVER_BASE} />}
        {page === "backup"     && <BackupManager baseUrl={SERVER_BASE} />}
        {page === "parallel"   && <ParallelView baseUrl={SERVER_BASE} />}
        {page === "federation" && <FederationView baseUrl={SERVER_BASE} />}
        {page === "cluster"    && <ClusterView baseUrl={SERVER_BASE} />}
        {page === "registry"   && <DatabaseRegistry baseUrl={SERVER_BASE} />}
      </main>
    </div>
  );
}
