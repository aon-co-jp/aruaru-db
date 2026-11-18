// ブランチ管理: 一覧・作成・切替・マージ
import { useEffect, useState } from "react";
import { invoke } from "../api/invoke";

interface Branch { name: string; headCommitId: string; isCurrent: boolean }
interface Result { success: boolean; message?: string; commitId?: string }

export default function BranchView({ serverUrl, onBranchChange }: { serverUrl: string; onBranchChange?: (b: string) => void }) {
  const [branches, setBranches] = useState<Branch[]>([]);
  const [name, setName] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = async () => {
    try {
      const bs = await invoke<Branch[]>("list_branches", { serverUrl });
      setBranches(bs);
      const cur = bs.find((b) => b.isCurrent);
      if (cur) onBranchChange?.(cur.name);
    } catch (e: any) {
      setMsg(String(e?.message ?? e));
    }
  };
  useEffect(() => { load(); }, [serverUrl]);

  const run = async (cmd: string, args: Record<string, unknown>) => {
    setBusy(true);
    setMsg(null);
    try {
      const r = await invoke<Result>(cmd, { serverUrl, ...args });
      setMsg(`${r.success ? "成功" : "失敗"}: ${r.message ?? ""}`);
      await load();
    } catch (e: any) {
      setMsg(String(e?.message ?? e));
    } finally {
      setBusy(false);
    }
  };

  const current = branches.find((b) => b.isCurrent)?.name;
  return (
    <div className="p-6 space-y-4">
      <h2 className="text-xl font-bold text-gray-100">ブランチ</h2>
      <div className="flex gap-2">
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="新しいブランチ名"
          className="bg-gray-800 text-gray-100 text-sm rounded px-3 py-1 border border-gray-700" />
        <button disabled={busy || !name.trim()} onClick={() => { run("create_branch", { name: name.trim() }); setName(""); }}
          className="text-sm px-3 py-1 rounded bg-blue-600 text-white disabled:opacity-40">作成</button>
      </div>
      {msg && <p className="text-sm text-gray-300">{msg}</p>}
      <table className="w-full text-sm">
        <thead><tr className="text-left text-gray-400"><th className="py-1">名前</th><th>先頭コミット</th><th></th></tr></thead>
        <tbody>
          {branches.map((b) => (
            <tr key={b.name} className="border-t border-gray-800 text-gray-200">
              <td className="py-1">{b.name}{b.isCurrent && <span className="ml-2 text-xs text-green-400">現在</span>}</td>
              <td className="text-yellow-400">{b.headCommitId?.slice(0, 8)}</td>
              <td className="text-right space-x-2">
                {!b.isCurrent && <button disabled={busy} onClick={() => run("checkout_branch", { branch: b.name })} className="text-xs px-2 py-1 rounded bg-gray-700">切替</button>}
                {!b.isCurrent && current && <button disabled={busy} onClick={() => run("merge_branch", { from: b.name })} className="text-xs px-2 py-1 rounded bg-gray-700">{current} へマージ</button>}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
