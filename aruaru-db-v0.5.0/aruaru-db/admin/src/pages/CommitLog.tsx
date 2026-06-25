// コミットログページ - Git log ライクな表示

import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface CommitEntry {
  id: string;
  shortId: string;
  author: string;
  message: string;
  timestamp: string;
  rootHash: string;
}

export default function CommitLog({ serverUrl }: { serverUrl: string }) {
  const [commits, setCommits] = useState<CommitEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [limit, setLimit] = useState(50);

  const fetchLog = async () => {
    setLoading(true);
    try {
      const result = await invoke<CommitEntry[]>("get_commit_log", {
        serverUrl,
        limit,
      });
      setCommits(result);
    } catch (e) {
      console.error("Failed to fetch commit log:", e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchLog();
  }, [limit]);

  return (
    <div className="p-6">
      <div className="flex items-center justify-between mb-6">
        <h2 className="text-xl font-bold text-gray-100">📝 コミットログ</h2>
        <div className="flex items-center gap-3">
          <select
            value={limit}
            onChange={(e) => setLimit(Number(e.target.value))}
            className="bg-gray-800 text-gray-300 border border-gray-700 rounded px-2 py-1 text-sm"
          >
            <option value={20}>20件</option>
            <option value={50}>50件</option>
            <option value={100}>100件</option>
          </select>
          <button
            onClick={fetchLog}
            className="px-3 py-1 bg-orange-500 hover:bg-orange-600 rounded text-sm text-white"
          >
            更新
          </button>
        </div>
      </div>

      {loading ? (
        <div className="text-gray-500 text-center py-12">読み込み中...</div>
      ) : commits.length === 0 ? (
        <div className="text-gray-600 text-center py-12">
          コミットがありません
        </div>
      ) : (
        <div className="space-y-1">
          {commits.map((commit, idx) => (
            <div
              key={commit.id}
              className="flex items-start gap-4 px-4 py-3 rounded-lg hover:bg-gray-800/50 group"
            >
              {/* グラフ線 */}
              <div className="flex flex-col items-center pt-1">
                <div className="w-3 h-3 rounded-full bg-orange-400 flex-shrink-0" />
                {idx < commits.length - 1 && (
                  <div className="w-0.5 h-full bg-gray-700 mt-1 min-h-[2rem]" />
                )}
              </div>

              {/* コミット情報 */}
              <div className="flex-1 min-w-0">
                <div className="flex items-baseline gap-3">
                  <code className="text-orange-400 text-sm font-mono">
                    {commit.shortId}
                  </code>
                  <span className="text-gray-200 text-sm truncate">
                    {commit.message}
                  </span>
                </div>
                <div className="flex items-center gap-3 mt-1 text-xs text-gray-500">
                  <span>👤 {commit.author}</span>
                  <span>🕐 {commit.timestamp}</span>
                  <code className="text-gray-600 hidden group-hover:inline">
                    {commit.rootHash.slice(0, 16)}...
                  </code>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
