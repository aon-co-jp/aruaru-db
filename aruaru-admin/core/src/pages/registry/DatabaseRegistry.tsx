// Tauri Admin: 対応DBレジストリ (150+件)
// DB-Engines 等から毎日クロールして自動更新される対応DB一覧。
// ステータス5段階・カテゴリ・ワイヤ・移行/バックアップ対応を表示。
import { useState, useEffect, useMemo, Fragment } from "react";
import { invoke } from "../../api/invoke";

type Status = "Ga" | "Beta" | "PgCompatible" | "ReadOnly" | "Planned";

interface DbEntry {
  id: string;
  name: string;
  category: string;
  wire: string;
  status: Status;
  migrate: string;
  backup: string;
  rank: number | null;
  score: number | null;
  updated_at: string | null;
}

interface Summary {
  total: number; ga: number; beta: number;
  pg_compatible: number; read_only: number; planned: number;
  postgres_wire: number;
}

const STATUS_META: Record<Status, { label: string; cls: string }> = {
  Ga:           { label: "GA",          cls: "bg-green-500/20 text-green-300" },
  Beta:         { label: "Beta",        cls: "bg-blue-500/20 text-blue-300" },
  PgCompatible: { label: "PG互換接続可", cls: "bg-orange-500/20 text-orange-300" },
  ReadOnly:     { label: "読取専用",     cls: "bg-purple-500/20 text-purple-300" },
  Planned:      { label: "計画中",       cls: "bg-gray-700 text-gray-400" },
};

// サーバ未接続時の確認用サンプル（実際は150+件をサーバから取得）
const SAMPLE: DbEntry[] = [
  { id: "postgresql", name: "PostgreSQL", category: "Relational", wire: "Postgres", status: "Ga", migrate: "PgWire", backup: "Full", rank: 4, score: 620.5, updated_at: "2026-06-22T00:00:00Z" },
  { id: "cockroachdb", name: "CockroachDB", category: "Relational", wire: "Postgres", status: "PgCompatible", migrate: "PgWire", backup: "Full", rank: 27, score: 30.1, updated_at: "2026-06-22T00:00:00Z" },
  { id: "mysql", name: "MySQL", category: "Relational", wire: "MySQL", status: "Beta", migrate: "MySqlWire", backup: "Full", rank: 2, score: 1010.0, updated_at: null },
  { id: "snowflake", name: "Snowflake", category: "Relational", wire: "Proprietary", status: "ReadOnly", migrate: "Parquet", backup: "Snapshot", rank: 11, score: 120.0, updated_at: null },
  { id: "oracle", name: "Oracle", category: "Relational", wire: "Oracle", status: "Planned", migrate: "Dump", backup: "Snapshot", rank: 1, score: 1240.0, updated_at: null },
];

export default function DatabaseRegistry({ baseUrl }: { baseUrl: string }) {
  const [entries, setEntries] = useState<DbEntry[]>([]);
  const [summary, setSummary] = useState<Summary | null>(null);
  const [usingSample, setUsingSample] = useState(false);
  const [q, setQ] = useState("");
  const [statusFilter, setStatusFilter] = useState<Status | "all">("all");
  const [crawling, setCrawling] = useState(false);
  const [crawlMsg, setCrawlMsg] = useState<string | null>(null);
  const [testId, setTestId] = useState<string | null>(null);
  const [testUri, setTestUri] = useState("");
  const [testMsg, setTestMsg] = useState<string | null>(null);

  const load = async () => {
    try {
      const [list, sum] = await Promise.all([
        invoke<DbEntry[]>("list_registry", { baseUrl }),
        invoke<Summary>("registry_summary", { baseUrl }),
      ]);
      setEntries(list); setSummary(sum); setUsingSample(false);
    } catch {
      setEntries(SAMPLE);
      setSummary({ total: 159, ga: 1, beta: 18, pg_compatible: 28, read_only: 16, planned: 96, postgres_wire: 28 });
      setUsingSample(true);
    }
  };
  useEffect(() => { load(); }, [baseUrl]);

  const runCrawl = async () => {
    setCrawling(true); setCrawlMsg(null);
    try {
      const res = await invoke<{ success: boolean; report?: any; message?: string }>("registry_crawl", { baseUrl });
      if (res.success && res.report) {
        setCrawlMsg(`✓ ${res.report.crawled}件取得 / ${res.report.matched}件照合`);
        await load();
      } else {
        setCrawlMsg(`✗ ${res.message ?? "失敗"}`);
      }
    } catch (e) {
      setCrawlMsg(`✗ クロール失敗（サーバ未接続）: ${e}`);
    } finally {
      setCrawling(false);
    }
  };

  const runTest = async (id: string) => {
    setTestMsg("テスト中…");
    try {
      const r = await invoke<{ ok: boolean; message: string; server_version?: string }>("registry_test", { baseUrl, id, uri: testUri });
      setTestMsg(r.ok ? `✓ ${r.message}${r.server_version ? ` (v${r.server_version})` : ""}` : `✗ ${r.message}`);
    } catch (e) { setTestMsg(`✗ ${e}`); }
  };

  const filtered = useMemo(() => {
    const needle = q.toLowerCase();
    return entries.filter((e) =>
      (statusFilter === "all" || e.status === statusFilter) &&
      (needle === "" || e.name.toLowerCase().includes(needle) || e.category.toLowerCase().includes(needle))
    );
  }, [entries, q, statusFilter]);

  return (
    <div className="p-6 space-y-5 max-w-5xl">
      <div className="flex items-center justify-between">
        <h2 className="text-xl font-bold text-gray-100">🗃️ 対応DBレジストリ</h2>
        <div className="flex items-center gap-3">
          {usingSample && <span className="text-xs text-yellow-500/80 bg-yellow-500/10 px-2 py-1 rounded">サンプル表示</span>}
          <button onClick={runCrawl} disabled={crawling}
            className="text-sm px-3 py-1.5 bg-orange-500 hover:bg-orange-600 disabled:opacity-40 rounded text-white">
            {crawling ? "クロール中…" : "今すぐクロール 🔄"}
          </button>
        </div>
      </div>
      <p className="text-xs text-gray-500">
        DB-Engines 等から毎日自動クロールし、ランキングと対応状況を更新します。
        {crawlMsg && <span className="ml-2 text-gray-400">{crawlMsg}</span>}
      </p>

      {/* サマリ */}
      {summary && (
        <div className="grid grid-cols-6 gap-2">
          <Stat label="総数" value={summary.total} cls="text-gray-200" />
          <Stat label="GA" value={summary.ga} cls="text-green-400" />
          <Stat label="Beta" value={summary.beta} cls="text-blue-400" />
          <Stat label="PG互換" value={summary.pg_compatible} cls="text-orange-400" />
          <Stat label="読取専用" value={summary.read_only} cls="text-purple-400" />
          <Stat label="計画中" value={summary.planned} cls="text-gray-500" />
        </div>
      )}

      {/* 検索・フィルタ */}
      <div className="flex items-center gap-2">
        <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="DB名・カテゴリで検索…"
          className="flex-1 bg-gray-800 border border-gray-700 rounded px-3 py-1.5 text-sm text-gray-200" />
        <select value={statusFilter} onChange={(e) => setStatusFilter(e.target.value as any)}
          className="bg-gray-800 border border-gray-700 rounded px-3 py-1.5 text-sm text-gray-300">
          <option value="all">全ステータス</option>
          <option value="Ga">GA</option>
          <option value="Beta">Beta</option>
          <option value="PgCompatible">PG互換接続可</option>
          <option value="ReadOnly">読取専用</option>
          <option value="Planned">計画中</option>
        </select>
      </div>

      {/* 一覧 */}
      <div className="bg-gray-900 rounded-xl overflow-hidden">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-xs text-gray-500 border-b border-gray-800">
              <th className="text-left px-3 py-2 w-12">#</th>
              <th className="text-left px-3 py-2">データベース</th>
              <th className="text-left px-3 py-2">分類</th>
              <th className="text-left px-3 py-2">ワイヤ</th>
              <th className="text-left px-3 py-2">ステータス</th>
              <th className="text-left px-3 py-2">移行/BK</th>
              <th className="text-right px-3 py-2">スコア</th>
              <th className="px-3 py-2"></th>
            </tr>
          </thead>
          <tbody>
            {filtered.map((e) => (
              <Fragment key={e.id}>
                <tr className="border-b border-gray-800/50 hover:bg-gray-850">
                  <td className="px-3 py-2 text-gray-600">{e.rank ?? "—"}</td>
                  <td className="px-3 py-2 text-gray-200 font-medium">{e.name}</td>
                  <td className="px-3 py-2 text-gray-500 text-xs">{e.category}</td>
                  <td className="px-3 py-2 text-gray-500 text-xs">{e.wire}</td>
                  <td className="px-3 py-2">
                    <span className={`text-xs px-2 py-0.5 rounded ${STATUS_META[e.status].cls}`}>
                      {STATUS_META[e.status].label}
                    </span>
                  </td>
                  <td className="px-3 py-2 text-xs text-gray-500">{e.migrate} / {e.backup}</td>
                  <td className="px-3 py-2 text-right text-gray-400 font-mono text-xs">{e.score?.toFixed(1) ?? "—"}</td>
                  <td className="px-3 py-2 text-right">
                    {["Postgres", "MySQL", "Mongo", "Cql"].includes(e.wire) && (
                      <button onClick={() => { setTestId(testId === e.id ? null : e.id); setTestMsg(null); setTestUri(""); }}
                        className="text-xs text-orange-400/80 hover:text-orange-300">接続テスト</button>
                    )}
                  </td>
                </tr>
                {testId === e.id && (
                  <tr className="bg-gray-950">
                    <td colSpan={8} className="px-3 py-3">
                      <div className="flex items-center gap-2">
                        <input value={testUri} onChange={(ev) => setTestUri(ev.target.value)}
                          placeholder={
                            e.wire === "MySQL" ? "mysql://user:pass@host:3306/db"
                            : e.wire === "Mongo" ? "mongodb://host:27017/db"
                            : e.wire === "Cql" ? "host:9042"
                            : "postgres://user:pass@host:5432/db"
                          }
                          className="flex-1 bg-gray-800 border border-gray-700 rounded px-3 py-1.5 text-xs text-gray-200 font-mono" />
                        <button onClick={() => runTest(e.id)} disabled={!testUri}
                          className="px-3 py-1.5 bg-orange-500 hover:bg-orange-600 disabled:opacity-40 rounded text-xs text-white">テスト</button>
                      </div>
                      {testMsg && <p className="text-xs text-gray-400 mt-2">{testMsg}</p>}
                      <p className="text-[11px] text-gray-600 mt-1">
                        {e.name} に実際に接続して確認します（{e.wire} ワイヤ）。
                      </p>
                    </td>
                  </tr>
                )}
              </Fragment>
            ))}
          </tbody>
        </table>
        {filtered.length === 0 && <p className="text-center text-gray-600 text-sm py-8">該当するDBがありません</p>}
      </div>
      <p className="text-xs text-gray-600">{filtered.length} 件表示中 / 全 {entries.length} 件</p>
    </div>
  );
}

function Stat({ label, value, cls }: { label: string; value: number; cls: string }) {
  return (
    <div className="bg-gray-900 rounded-lg p-3 text-center">
      <div className={`text-xl font-bold ${cls}`}>{value}</div>
      <div className="text-xs text-gray-600">{label}</div>
    </div>
  );
}
