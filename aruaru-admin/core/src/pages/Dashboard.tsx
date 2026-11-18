// Dashboard page - aruaru-DB Admin
import { useEffect, useState } from "react";
import { invoke } from "../api/invoke";

interface Commit { id: string; shortId: string; author: string; message: string; timestamp: string }
interface Branch { name: string; headCommitId: string; isCurrent: boolean }

export default function Dashboard({ serverUrl, onBranchChange }: { serverUrl: string; onBranchChange?: (b: string) => void }) {
  const [alive, setAlive] = useState<boolean | null>(null);
  const [branches, setBranches] = useState<Branch[]>([]);
  const [commits, setCommits] = useState<Commit[]>([]);
  const [error, setError] = useState<string | null>(null);

  const load = async () => {
    setError(null);
    try {
      setAlive(await invoke<boolean>("ping_server", { serverUrl }));
      const bs = await invoke<Branch[]>("list_branches", { serverUrl });
      setBranches(bs);
      const cur = bs.find((b) => b.isCurrent);
      if (cur) onBranchChange?.(cur.name);
      setCommits(await invoke<Commit[]>("get_commit_log", { serverUrl, limit: 5 }));
    } catch (e: any) {
      setAlive(false);
      setError(String(e?.message ?? e));
    }
  };
  useEffect(() => { load(); }, [serverUrl]);

  const current = branches.find((b) => b.isCurrent);
  return (
    <div className="p-6 space-y-4">
      <div className="flex items-center gap-3">
        <h2 className="text-xl font-bold text-gray-100">Dashboard</h2>
        <button onClick={load} className="text-xs px-2 py-1 rounded bg-gray-700 text-gray-200">再読み込み</button>
      </div>
      {error && <p className="text-red-400 text-sm">{error}</p>}
      <div className="grid grid-cols-3 gap-4 text-sm">
        <div className="bg-gray-800 rounded p-4">
          <div className="text-gray-400">サーバー</div>
          <div className={alive ? "text-green-400" : "text-red-400"}>{alive === null ? "確認中…" : alive ? "接続OK" : "接続できません"}</div>
        </div>
        <div className="bg-gray-800 rounded p-4">
          <div className="text-gray-400">現在のブランチ</div>
          <div className="text-gray-100">{current?.name ?? "-"}</div>
        </div>
        <div className="bg-gray-800 rounded p-4">
          <div className="text-gray-400">ブランチ数</div>
          <div className="text-gray-100">{branches.length}</div>
        </div>
      </div>
      <div className="bg-gray-800 rounded p-4">
        <div className="text-gray-400 text-sm mb-2">最近のコミット</div>
        {commits.length === 0 ? <p className="text-gray-500 text-sm">コミットはありません</p> : (
          <ul className="space-y-1 text-sm">
            {commits.map((c) => (
              <li key={c.id} className="text-gray-200"><span className="text-yellow-400 mr-2">{c.shortId}</span>{c.message}<span className="text-gray-500 ml-2">{c.author}</span></li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
