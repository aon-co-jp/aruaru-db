/**
 * aruaru-DB Node.js / TypeScript ドライバー
 *
 * パッケージ名: @aruaru-db/nodejs
 * npm install @aruaru-db/nodejs
 * 内部依存: postgres (postgres.js)
 *
 * aruaru-DB は PostgreSQL ワイヤ互換のため、postgres.js がそのまま使えます。
 * このドライバーは Git-on-SQL 操作を型付き API で包むラッパーです。
 */

import postgres, { Sql } from "postgres";

// ── 型定義 ───────────────────────────────────────────────────

export interface CommitEntry {
  id:        string;
  short_id:  string;
  author:    string;
  message:   string;
  timestamp: string;
  root_hash: string;
}

export interface DiffStat {
  added:    number;
  removed:  number;
  modified: number;
}

export interface AruaruDBConfig {
  host?:     string;  // 既定: "localhost"
  port?:     number;  // 既定: 5432
  database?: string;  // 既定: "aruaru"
  username?: string;  // 既定: "root"
  password?: string;
}

// ── メインクラス ──────────────────────────────────────────────

/**
 * aruaru-DB Node.js ドライバー
 *
 * @example
 * ```ts
 * import { AruaruDBDriver } from "@aruaru-db/nodejs";
 *
 * const db = new AruaruDBDriver({ host: "localhost" });
 * await db.branch("feature/my-feature");
 * await db.execute`CREATE TABLE tasks (id INT, title TEXT)`;
 * const commitId = await db.commit("Add tasks table");
 * console.log("Committed:", commitId);
 * await db.end();
 * ```
 */
export class AruaruDBDriver {
  readonly sql: Sql;

  constructor(config: AruaruDBConfig = {}) {
    this.sql = postgres({
      host:     config.host     ?? "localhost",
      port:     config.port     ?? 5432,
      database: config.database ?? "aruaru",
      username: config.username ?? "root",
      password: config.password ?? "",
    });
  }

  /** URL 文字列から接続 ("postgres://root@localhost:5432/aruaru") */
  static fromUrl(url: string): AruaruDBDriver {
    const driver = Object.create(AruaruDBDriver.prototype) as AruaruDBDriver;
    (driver as any).sql = postgres(url);
    return driver;
  }

  // ── Git-on-SQL ───────────────────────────────────────────

  /** ブランチを作成する */
  async branch(name: string): Promise<void> {
    await this.sql`SELECT aruaru_branch(${name})`;
  }

  /** ブランチを切り替える */
  async checkout(name: string): Promise<void> {
    await this.sql`SELECT aruaru_checkout(${name})`;
  }

  /** 現在のブランチ名を取得する */
  async currentBranch(): Promise<string> {
    const [row] = await this.sql<[{ aruaru_current_branch: string }]>
      `SELECT aruaru_current_branch()`;
    return row.aruaru_current_branch;
  }

  /** コミットしてコミット ID を返す */
  async commit(message: string): Promise<string> {
    const [row] = await this.sql<[{ aruaru_commit: string }]>
      `SELECT aruaru_commit(${message})`;
    return row.aruaru_commit;
  }

  /** fast-forward マージしてコミット ID を返す */
  async merge(fromBranch: string): Promise<string> {
    const [row] = await this.sql<[{ aruaru_merge: string }]>
      `SELECT aruaru_merge(${fromBranch})`;
    return row.aruaru_merge;
  }

  /** コミットログを取得する */
  async log(limit = 20): Promise<CommitEntry[]> {
    return this.sql<CommitEntry[]>
      `SELECT * FROM aruaru_log ORDER BY timestamp DESC LIMIT ${limit}`;
  }

  /** 2ブランチ間の差分統計を取得する */
  async diff(from: string, to: string): Promise<DiffStat> {
    const [row] = await this.sql<[DiffStat]>
      `SELECT * FROM aruaru_diff(${from}, ${to})`;
    return row;
  }

  // ── 汎用 SQL ─────────────────────────────────────────────

  /**
   * テンプレートリテラルで SQL を実行する (推奨・SQLインジェクション安全)
   *
   * @example
   * await db.execute`INSERT INTO tasks (id, title) VALUES (${1}, ${"Hello"})`;
   */
  get execute() {
    return this.sql;
  }

  /** 接続を閉じる */
  async end(): Promise<void> {
    await this.sql.end();
  }
}

// ── 使用例 ───────────────────────────────────────────────────
async function example() {
  const db = new AruaruDBDriver({ host: "localhost" });

  await db.branch("feature/nodejs-test");
  await db.execute`CREATE TABLE IF NOT EXISTS products (id INT, name TEXT, price NUMERIC)`;
  await db.execute`INSERT INTO products VALUES (${1}, ${"Widget"}, ${9.99})`;

  const commitId = await db.commit("Add products via aruaru-db-nodejs");
  console.log("Committed:", commitId);

  const log = await db.log(5);
  log.forEach(e => console.log(`${e.short_id}  ${e.author}: ${e.message}`));

  await db.end();
}
