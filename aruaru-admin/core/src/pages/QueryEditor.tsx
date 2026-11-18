// クエリ実行: SQL(execSql)と GraphQL の問い合わせ
import { useState } from "react";
import { invoke } from "../api/invoke";

export default function QueryEditor({ serverUrl }: { serverUrl: string; onBranchChange?: (b: string) => void }) {
  const [mode, setMode] = useState<"sql" | "graphql">("sql");
  const [text, setText] = useState("");
  const [autoCommit, setAutoCommit] = useState(false);
  const [out, setOut] = useState<string>("");
  const [busy, setBusy] = useState(false);

  const run = async () => {
    setBusy(true);
    setOut("");
    try {
      const r = mode === "sql"
        ? await invoke("exec_sql", { serverUrl, sql: text, autoCommit })
        : await invoke("graphql_query", { serverUrl, query: text });
      setOut(JSON.stringify(r, null, 2));
    } catch (e: any) {
      setOut(String(e?.message ?? e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="p-6 space-y-3">
      <h2 className="text-xl font-bold text-gray-100">クエリ</h2>
      <div className="flex gap-2 text-sm">
        {(["sql", "graphql"] as const).map((m) => (
          <button key={m} onClick={() => setMode(m)} className={`px-3 py-1 rounded ${mode === m ? "bg-blue-600 text-white" : "bg-gray-700 text-gray-200"}`}>{m === "sql" ? "SQL" : "GraphQL"}</button>
        ))}
        {mode === "sql" && (
          <label className="flex items-center gap-1 text-gray-300"><input type="checkbox" checked={autoCommit} onChange={(e) => setAutoCommit(e.target.checked)} />自動コミット</label>
        )}
      </div>
      <textarea value={text} onChange={(e) => setText(e.target.value)} rows={8} spellCheck={false}
        className="w-full bg-gray-800 text-gray-100 text-sm font-mono rounded p-3 border border-gray-700"
        placeholder={mode === "sql" ? "INSERT INTO ... / CREATE TABLE ..." : "query { branches { name } }"} />
      <button disabled={busy || !text.trim()} onClick={run} className="text-sm px-4 py-1 rounded bg-blue-600 text-white disabled:opacity-40">実行</button>
      {out && <pre className="bg-gray-900 text-gray-200 text-xs rounded p-3 overflow-auto max-h-96">{out}</pre>}
    </div>
  );
}
