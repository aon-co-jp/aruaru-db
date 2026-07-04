// Tauri Admin: 分散DB統合 (フェデレーション) ページ
// 外部DB(他の aruaru / PostgreSQL / CockroachDB / Snowflake / MySQL) を
// 統合ソースとして登録し、複数DBをまたぐ横断クエリを実行する。
import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

type SourceKind = "aruaru" | "postgres" | "cockroach" | "snowflake" | "mysql";

interface FederatedSource {
  name: string;
  kind: SourceKind;
  uri: string;
  read_only: boolean;
  pushdown: boolean;
  status?: "online" | "offline" | "unknown";
  table_count?: number;
}

interface QueryResult {
  columns: string[];
  rows: string[][];
  sources_touched: string[]; // 横断したソース名
  elapsed_ms: number;
}

const KIND_META: Record<SourceKind, { icon: string; label: string; placeholder: string }> = {
  aruaru:    { icon: "🦀", label: "aruaru-DB",   placeholder: "aruaru://host:5432/db" },
  postgres:  { icon: "🐘", label: "PostgreSQL",  placeholder: "postgres://user:pass@host:5432/db" },
  cockroach: { icon: "🪳", label: "CockroachDB", placeholder: "postgresql://host:26257/db" },
  snowflake: { icon: "❄️",  label: "Snowflake",   placeholder: "snowflake://account/db/schema" },
  mysql:     { icon: "🐬", label: "MySQL",       placeholder: "mysql://user:pass@host:3306/db" },
};

const SAMPLE_SOURCES: FederatedSource[] = [
  { name: "analytics_pg", kind: "postgres", uri: "postgres://…/analytics", read_only: true, pushdown: true, status: "online", table_count: 42 },
  { name: "warehouse_sf", kind: "snowflake", uri: "snowflake://…/WH", read_only: true, pushdown: false, status: "online", table_count: 128 },
  { name: "edge_tokyo", kind: "aruaru", uri: "aruaru://tokyo-edge:5432/main", read_only: false, pushdown: true, status: "online", table_count: 17 },
];

export default function FederationView({ baseUrl }: { baseUrl: string }) {
  const [sources, setSources] = useState<FederatedSource[]>([]);
  const [usingSample, setUsingSample] = useState(false);
  const [showAdd, setShowAdd] = useState(false);

  // 追加フォーム
  const [name, setName] = useState("");
  const [kind, setKind] = useState<SourceKind>("postgres");
  const [uri, setUri] = useState("");
  const [readOnly, setReadOnly] = useState(true);
  const [pushdown, setPushdown] = useState(true);
  const [testResult, setTestResult] = useState<string | null>(null);

  // 横断クエリ
  const [sql, setSql] = useState(
    "SELECT l.id, l.name, r.total\nFROM local.users l\nJOIN analytics_pg.orders r ON r.user_id = l.id\nLIMIT 100"
  );
  const [result, setResult] = useState<QueryResult | null>(null);
  const [queryErr, setQueryErr] = useState<string | null>(null);
  const [querying, setQuerying] = useState(false);

  const fetchSources = async () => {
    try {
      const list = await invoke<FederatedSource[]>("list_federated_sources", { baseUrl });
      setSources(list);
      setUsingSample(false);
    } catch {
      setSources(SAMPLE_SOURCES);
      setUsingSample(true);
    }
  };
  useEffect(() => { fetchSources(); }, [baseUrl]);

  const testConnection = async () => {
    setTestResult("接続テスト中…");
    try {
      const res = await invoke<{ ok: boolean; message: string }>("test_federated_source", { baseUrl, kind, uri });
      setTestResult(res.ok ? `✓ 接続成功: ${res.message}` : `✗ 失敗: ${res.message}`);
    } catch (e) {
      setTestResult(`✗ ${e}`);
    }
  };

  const addSource = async () => {
    try {
      await invoke("register_federated_source", {
        baseUrl,
        source: { name, kind, uri, read_only: readOnly, pushdown },
      });
    } catch { /* サンプルモードでもUI上は追加 */ }
    setSources((s) => [
      ...s,
      { name, kind, uri, read_only: readOnly, pushdown, status: "unknown" },
    ]);
    setShowAdd(false);
    setName(""); setUri(""); setTestResult(null);
  };

  const dropSource = async (n: string) => {
    if (!confirm(`統合ソース「${n}」を削除しますか？`)) return;
    try { await invoke("drop_federated_source", { baseUrl, name: n }); } catch {}
    setSources((s) => s.filter((x) => x.name !== n));
  };

  const runQuery = async () => {
    setQuerying(true); setResult(null); setQueryErr(null);
    try {
      const res = await invoke<QueryResult>("federated_query", { baseUrl, sql });
      setResult(res);
    } catch (e) {
      setQueryErr(String(e));
    } finally {
      setQuerying(false);
    }
  };

  return (
    <div className="p-6 space-y-6 max-w-4xl">
      <div className="flex items-center justify-between">
        <h2 className="text-xl font-bold text-gray-100">🌐 分散DB統合（フェデレーション）</h2>
        {usingSample && (
          <span className="text-xs text-yellow-500/80 bg-yellow-500/10 px-2 py-1 rounded">
            サンプル表示（サーバ未接続）
          </span>
        )}
      </div>

      {/* ── 登録済みソース ── */}
      <section className="bg-gray-900 rounded-xl p-5">
        <div className="flex items-center justify-between mb-4">
          <h3 className="font-semibold text-gray-200">統合ソース</h3>
          <button onClick={() => setShowAdd(!showAdd)}
            className="text-sm px-3 py-1.5 bg-orange-500 hover:bg-orange-600 rounded text-white">
            {showAdd ? "閉じる" : "+ ソース追加"}
          </button>
        </div>

        {showAdd && (
          <div className="bg-gray-950 rounded-lg p-4 mb-4 space-y-3 border border-gray-800">
            <div className="grid grid-cols-5 gap-2">
              {(Object.keys(KIND_META) as SourceKind[]).map((k) => (
                <button key={k} onClick={() => setKind(k)}
                  className={`px-2 py-2 rounded text-xs text-center ${
                    kind === k ? "bg-orange-500/20 text-orange-300 border border-orange-500" : "bg-gray-800 text-gray-400 border border-transparent"
                  }`}>
                  <div className="text-lg">{KIND_META[k].icon}</div>
                  {KIND_META[k].label}
                </button>
              ))}
            </div>
            <div className="grid grid-cols-2 gap-2">
              <input value={name} onChange={(e) => setName(e.target.value)} placeholder="ソース名 (例: analytics_pg)"
                className="bg-gray-800 border border-gray-700 rounded px-3 py-1.5 text-sm text-gray-200 font-mono" />
              <input value={uri} onChange={(e) => setUri(e.target.value)} placeholder={KIND_META[kind].placeholder}
                className="bg-gray-800 border border-gray-700 rounded px-3 py-1.5 text-sm text-gray-200 font-mono" />
            </div>
            <div className="flex items-center gap-5">
              <label className="flex items-center gap-2 text-sm text-gray-400 cursor-pointer">
                <input type="checkbox" checked={readOnly} onChange={(e) => setReadOnly(e.target.checked)} className="accent-orange-500" />
                読み取り専用
              </label>
              <label className="flex items-center gap-2 text-sm text-gray-400 cursor-pointer">
                <input type="checkbox" checked={pushdown} onChange={(e) => setPushdown(e.target.checked)} className="accent-orange-500" />
                プッシュダウン最適化
              </label>
            </div>
            <div className="flex items-center gap-3">
              <button onClick={testConnection}
                className="px-3 py-1.5 bg-gray-700 hover:bg-gray-600 rounded text-sm text-gray-200">接続テスト</button>
              <button onClick={addSource} disabled={!name || !uri}
                className="px-4 py-1.5 bg-orange-500 hover:bg-orange-600 disabled:opacity-40 rounded text-sm text-white">登録</button>
              {testResult && <span className="text-xs text-gray-400">{testResult}</span>}
            </div>
          </div>
        )}

        {sources.length === 0 ? (
          <p className="text-gray-600 text-sm text-center py-6">統合ソースが登録されていません</p>
        ) : (
          <div className="space-y-2">
            {sources.map((s) => (
              <div key={s.name} className="flex items-center gap-3 bg-gray-950 rounded-lg p-3 border border-gray-800">
                <span className="text-2xl">{KIND_META[s.kind].icon}</span>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="text-gray-200 font-medium">{s.name}</span>
                    <span className="text-xs text-gray-600">{KIND_META[s.kind].label}</span>
                    {s.read_only && <span className="text-xs bg-gray-800 text-gray-500 px-1.5 py-0.5 rounded">RO</span>}
                    {s.pushdown && <span className="text-xs bg-blue-500/15 text-blue-400 px-1.5 py-0.5 rounded">pushdown</span>}
                  </div>
                  <div className="text-xs text-gray-600 truncate font-mono">{s.uri}</div>
                </div>
                {s.table_count != null && <span className="text-xs text-gray-500">{s.table_count} テーブル</span>}
                <StatusDot status={s.status} />
                <button onClick={() => dropSource(s.name)}
                  className="text-xs text-red-400/70 hover:text-red-400 px-2">削除</button>
              </div>
            ))}
          </div>
        )}
      </section>

      {/* ── 横断クエリ ── */}
      <section className="bg-gray-900 rounded-xl p-5 space-y-3">
        <h3 className="font-semibold text-gray-200">横断クエリ（フェデレーテッドクエリ）</h3>
        <p className="text-xs text-gray-600">
          ローカルテーブルは <code className="text-gray-400">local.テーブル名</code>、
          外部ソースは <code className="text-gray-400">ソース名.テーブル名</code> で参照できます。
        </p>
        <textarea value={sql} onChange={(e) => setSql(e.target.value)} rows={5}
          className="w-full bg-gray-950 border border-gray-700 rounded px-3 py-2 text-sm text-gray-200 font-mono" />
        <button onClick={runQuery} disabled={querying}
          className="px-5 py-2 bg-orange-500 hover:bg-orange-600 disabled:opacity-40 rounded text-white text-sm">
          {querying ? "実行中…" : "横断クエリ実行 🌐"}
        </button>

        {queryErr && <div className="text-sm text-red-400 bg-red-500/10 rounded p-3">{queryErr}</div>}

        {result && (
          <div className="space-y-2">
            <div className="flex items-center gap-2 text-xs text-gray-500">
              <span>{result.rows.length} 行</span>
              <span>·</span>
              <span>{result.elapsed_ms}ms</span>
              <span>·</span>
              <span>横断ソース: {result.sources_touched.join(" + ")}</span>
            </div>
            <div className="overflow-auto bg-gray-950 rounded-lg border border-gray-800">
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-xs text-gray-500 border-b border-gray-800">
                    {result.columns.map((c) => <th key={c} className="text-left px-3 py-2">{c}</th>)}
                  </tr>
                </thead>
                <tbody>
                  {result.rows.map((row, i) => (
                    <tr key={i} className="border-b border-gray-800/50">
                      {row.map((cell, j) => <td key={j} className="px-3 py-1.5 text-gray-300 font-mono text-xs">{cell}</td>)}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        )}
      </section>
    </div>
  );
}

function StatusDot({ status }: { status?: string }) {
  const color = status === "online" ? "bg-green-400" : status === "offline" ? "bg-red-500" : "bg-gray-600";
  const label = status === "online" ? "稼働中" : status === "offline" ? "停止" : "不明";
  return (
    <span className="flex items-center gap-1 text-xs text-gray-500">
      <span className={`w-2 h-2 rounded-full ${color}`} />
      {label}
    </span>
  );
}
