// Tauri Admin: 分散並列化ページ
// 並列クエリの設定・分散実行プランの可視化・実行中ジョブの監視
import { useState, useEffect } from "react";
import { invoke } from "../../api/invoke";

interface ParallelConfig {
  max_parallelism: number;
  worker_threads_per_node: number;
  enable_parallel_scan: boolean;
  enable_parallel_aggregate: boolean;
  enable_shuffle_join: boolean;
  shuffle_partitions: number;
  broadcast_threshold_mb: number;
}

interface PlanFragment {
  id: number;
  op: string;          // "ParallelScan" | "HashAggregate" | "ShuffleExchange" ...
  parallelism: number; // このフラグメントの並列度
  node_ids: number[];  // 実行ノード
  est_rows: number;
  detail: string;
}

interface ParallelJob {
  job_id: string;
  sql: string;
  fragments_total: number;
  fragments_done: number;
  rows_processed: number;
  elapsed_ms: number;
  node_progress: { node_id: number; pct: number }[];
}

const DEFAULT_CONFIG: ParallelConfig = {
  max_parallelism: 8,
  worker_threads_per_node: 4,
  enable_parallel_scan: true,
  enable_parallel_aggregate: true,
  enable_shuffle_join: true,
  shuffle_partitions: 64,
  broadcast_threshold_mb: 32,
};

// サーバ未接続時に UI を確認するためのサンプルプラン
const SAMPLE_PLAN: PlanFragment[] = [
  { id: 4, op: "Gather (Coordinator)", parallelism: 1, node_ids: [1], est_rows: 1200, detail: "全フラグメントの結果を集約" },
  { id: 3, op: "HashAggregate", parallelism: 8, node_ids: [1, 2, 3], est_rows: 1200, detail: "GROUP BY region — 部分集計をマージ" },
  { id: 2, op: "ShuffleExchange", parallelism: 64, node_ids: [1, 2, 3], est_rows: 4_800_000, detail: "region でハッシュ再分配 (64 パーティション)" },
  { id: 1, op: "ParallelScan", parallelism: 8, node_ids: [1, 2, 3], est_rows: 4_800_000, detail: "orders を Range 並列スキャン (述語プッシュダウン)" },
];

const SAMPLE_JOBS: ParallelJob[] = [
  {
    job_id: "job_7f3a", sql: "SELECT region, SUM(amount) FROM orders GROUP BY region",
    fragments_total: 81, fragments_done: 54, rows_processed: 3_240_000, elapsed_ms: 1840,
    node_progress: [{ node_id: 1, pct: 72 }, { node_id: 2, pct: 65 }, { node_id: 3, pct: 60 }],
  },
];

export default function ParallelView({ baseUrl }: { baseUrl: string }) {
  const [tab, setTab] = useState<"config" | "explain" | "jobs">("config");
  const [config, setConfig] = useState<ParallelConfig>(DEFAULT_CONFIG);
  const [saved, setSaved] = useState(false);
  const [sql, setSql] = useState("SELECT region, SUM(amount) FROM orders GROUP BY region");
  const [plan, setPlan] = useState<PlanFragment[]>([]);
  const [jobs, setJobs] = useState<ParallelJob[]>([]);
  const [usingSample, setUsingSample] = useState(false);

  useEffect(() => {
    invoke<ParallelConfig>("get_parallel_config", { baseUrl })
      .then((c) => c && setConfig(c))
      .catch(() => {});
  }, [baseUrl]);

  // ジョブをポーリング
  useEffect(() => {
    if (tab !== "jobs") return;
    const poll = () =>
      invoke<ParallelJob[]>("list_parallel_jobs", { baseUrl })
        .then((j) => { setJobs(j); setUsingSample(false); })
        .catch(() => { setJobs(SAMPLE_JOBS); setUsingSample(true); });
    poll();
    const t = setInterval(poll, 2000);
    return () => clearInterval(t);
  }, [tab, baseUrl]);

  const saveConfig = async () => {
    try {
      await invoke("set_parallel_config", { baseUrl, config });
      setSaved(true);
      setTimeout(() => setSaved(false), 1500);
    } catch {
      setSaved(true);
      setTimeout(() => setSaved(false), 1500);
    }
  };

  const runExplain = async () => {
    try {
      const res = await invoke<{ fragments: PlanFragment[] }>("explain_distributed", { baseUrl, sql });
      setPlan(res.fragments ?? []);
      setUsingSample(false);
    } catch {
      setPlan(SAMPLE_PLAN);
      setUsingSample(true);
    }
  };

  const fmt = (n: number) => n.toLocaleString();

  return (
    <div className="p-6 space-y-6 max-w-4xl">
      <div className="flex items-center justify-between">
        <h2 className="text-xl font-bold text-gray-100">⚙️ 分散並列化</h2>
        {usingSample && (
          <span className="text-xs text-yellow-500/80 bg-yellow-500/10 px-2 py-1 rounded">
            サンプル表示（サーバ未接続）
          </span>
        )}
      </div>

      {/* タブ */}
      <div className="flex gap-1 border-b border-gray-800">
        {([["config", "並列設定"], ["explain", "実行プラン"], ["jobs", "実行中ジョブ"]] as const).map(
          ([id, label]) => (
            <button key={id} onClick={() => setTab(id)}
              className={`px-4 py-2 text-sm border-b-2 -mb-px transition-colors ${
                tab === id ? "border-orange-500 text-orange-300" : "border-transparent text-gray-500 hover:text-gray-300"
              }`}>
              {label}
            </button>
          )
        )}
      </div>

      {/* ── 並列設定 ── */}
      {tab === "config" && (
        <section className="bg-gray-900 rounded-xl p-5 space-y-5">
          <div className="grid grid-cols-2 gap-5">
            <Slider label="クエリ最大並列度" value={config.max_parallelism} min={1} max={64}
              onChange={(v) => setConfig({ ...config, max_parallelism: v })}
              hint="1 クエリを最大いくつのフラグメントに分割するか" />
            <Slider label="ノードあたりワーカースレッド" value={config.worker_threads_per_node} min={1} max={32}
              onChange={(v) => setConfig({ ...config, worker_threads_per_node: v })}
              hint="各ノードの実行スレッド数 (≒CPUコア数)" />
            <Slider label="シャッフル分割数" value={config.shuffle_partitions} min={8} max={256} step={8}
              onChange={(v) => setConfig({ ...config, shuffle_partitions: v })}
              hint="再分配時のパーティション数 (大きいほど均等・オーバーヘッド増)" />
            <Slider label="Broadcast 閾値 (MB)" value={config.broadcast_threshold_mb} min={1} max={512}
              onChange={(v) => setConfig({ ...config, broadcast_threshold_mb: v })}
              hint="これ以下のテーブルは全ノードに配布して shuffle を回避" />
          </div>

          <div className="space-y-2 pt-2 border-t border-gray-800">
            <Toggle label="並列スキャン (Parallel Scan)" checked={config.enable_parallel_scan}
              onChange={(b) => setConfig({ ...config, enable_parallel_scan: b })}
              hint="Range を複数ワーカーで同時スキャン" />
            <Toggle label="並列集計 (Parallel Aggregate)" checked={config.enable_parallel_aggregate}
              onChange={(b) => setConfig({ ...config, enable_parallel_aggregate: b })}
              hint="部分集計→マージの2段階で GROUP BY を並列化" />
            <Toggle label="シャッフルジョイン (Shuffle Join)" checked={config.enable_shuffle_join}
              onChange={(b) => setConfig({ ...config, enable_shuffle_join: b })}
              hint="大規模テーブル同士をハッシュ再分配して結合" />
          </div>

          <div className="flex items-center gap-3">
            <button onClick={saveConfig}
              className="px-6 py-2 bg-orange-500 hover:bg-orange-600 rounded text-white font-medium">
              設定を保存
            </button>
            {saved && <span className="text-green-400 text-sm">✓ 保存しました</span>}
          </div>
        </section>
      )}

      {/* ── 実行プラン ── */}
      {tab === "explain" && (
        <section className="space-y-4">
          <div className="bg-gray-900 rounded-xl p-5 space-y-3">
            <label className="block text-xs text-gray-500">SQL を入力して分散実行プランを表示</label>
            <textarea value={sql} onChange={(e) => setSql(e.target.value)} rows={3}
              className="w-full bg-gray-950 border border-gray-700 rounded px-3 py-2 text-sm text-gray-200 font-mono" />
            <button onClick={runExplain}
              className="px-5 py-2 bg-orange-500 hover:bg-orange-600 rounded text-white text-sm">
              EXPLAIN (分散) ⚙️
            </button>
          </div>

          {plan.length > 0 && (
            <div className="bg-gray-900 rounded-xl p-5">
              <h3 className="font-semibold text-gray-200 mb-4">分散実行プラン（下から上へ実行）</h3>
              <div className="space-y-2">
                {plan.map((f, i) => (
                  <div key={f.id} className="relative">
                    <div className="flex items-center gap-3 bg-gray-950 rounded-lg p-3 border border-gray-800">
                      <div className="text-xs text-gray-600 w-6">#{f.id}</div>
                      <div className="flex-1">
                        <div className="flex items-center gap-2">
                          <span className="text-orange-300 font-medium text-sm">{f.op}</span>
                          <span className="text-xs bg-blue-500/20 text-blue-300 px-2 py-0.5 rounded">
                            ×{f.parallelism} 並列
                          </span>
                          <span className="text-xs text-gray-600">
                            ノード {f.node_ids.join(", ")}
                          </span>
                        </div>
                        <div className="text-xs text-gray-500 mt-1">{f.detail}</div>
                      </div>
                      <div className="text-right text-xs text-gray-500">
                        ~{fmt(f.est_rows)} 行
                      </div>
                    </div>
                    {i < plan.length - 1 && (
                      <div className="flex justify-center text-gray-700 text-xs my-0.5">▲</div>
                    )}
                  </div>
                ))}
              </div>
            </div>
          )}
        </section>
      )}

      {/* ── 実行中ジョブ ── */}
      {tab === "jobs" && (
        <section className="space-y-3">
          {jobs.length === 0 ? (
            <p className="text-gray-600 text-sm text-center py-10">実行中の並列ジョブはありません</p>
          ) : (
            jobs.map((j) => (
              <div key={j.job_id} className="bg-gray-900 rounded-xl p-5 space-y-3">
                <div className="flex items-center justify-between">
                  <span className="font-mono text-orange-400 text-sm">{j.job_id}</span>
                  <span className="text-xs text-gray-500">{(j.elapsed_ms / 1000).toFixed(1)}s 経過</span>
                </div>
                <code className="block text-xs text-gray-400 bg-gray-950 rounded px-3 py-2">{j.sql}</code>
                <div className="flex items-center gap-2 text-xs text-gray-500">
                  <span>フラグメント {j.fragments_done}/{j.fragments_total}</span>
                  <span>·</span>
                  <span>{fmt(j.rows_processed)} 行処理</span>
                </div>
                <div className="space-y-1.5">
                  {j.node_progress.map((np) => (
                    <div key={np.node_id} className="flex items-center gap-2">
                      <span className="text-xs text-gray-500 w-16">ノード {np.node_id}</span>
                      <div className="flex-1 bg-gray-800 rounded-full h-1.5">
                        <div className="bg-orange-500 h-1.5 rounded-full transition-all"
                          style={{ width: `${np.pct}%` }} />
                      </div>
                      <span className="text-xs text-gray-500 w-10 text-right">{np.pct}%</span>
                    </div>
                  ))}
                </div>
              </div>
            ))
          )}
        </section>
      )}
    </div>
  );
}

// ── 小物コンポーネント ──────────────────────────────────────────

function Slider({ label, value, min, max, step = 1, onChange, hint }: {
  label: string; value: number; min: number; max: number; step?: number;
  onChange: (v: number) => void; hint?: string;
}) {
  return (
    <div>
      <div className="flex items-center justify-between mb-1">
        <label className="text-sm text-gray-300">{label}</label>
        <span className="text-sm text-orange-400 font-mono">{value}</span>
      </div>
      <input type="range" min={min} max={max} step={step} value={value}
        onChange={(e) => onChange(Number(e.target.value))} className="w-full accent-orange-500" />
      {hint && <p className="text-xs text-gray-600 mt-0.5">{hint}</p>}
    </div>
  );
}

function Toggle({ label, checked, onChange, hint }: {
  label: string; checked: boolean; onChange: (b: boolean) => void; hint?: string;
}) {
  return (
    <label className="flex items-start gap-3 cursor-pointer py-1">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)}
        className="accent-orange-500 mt-0.5" />
      <div>
        <div className="text-sm text-gray-300">{label}</div>
        {hint && <div className="text-xs text-gray-600">{hint}</div>}
      </div>
    </label>
  );
}
