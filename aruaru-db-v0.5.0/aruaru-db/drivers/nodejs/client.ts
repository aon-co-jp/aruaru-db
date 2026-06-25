// aruaru-db Node.js / TypeScript Client
// npm install @aruaru-db/client
// 内部依存: postgres (postgres.js) または pg

import postgres from "postgres";

// ── 接続 ─────────────────────────────────────────────────────
const sql = postgres({
  host: "localhost",
  port: 5432,
  database: "aruaru",
  username: "root",
  password: "",
});

// ── Git-on-SQL ラッパー ────────────────────────────────────────
export class AruaruDB {
  constructor(private sql: postgres.Sql) {}

  async branch(name: string) {
    await this.sql`SELECT aruaru_branch(${name})`;
  }

  async checkout(name: string) {
    await this.sql`SELECT aruaru_checkout(${name})`;
  }

  async commit(author: string, message: string) {
    const [row] = await this.sql`SELECT aruaru_commit(${message}) as commit_id`;
    return row.commit_id as string;
  }

  async log(limit = 20) {
    return this.sql`SELECT * FROM aruaru_log ORDER BY timestamp DESC LIMIT ${limit}`;
  }

  async diff(from: string, to: string) {
    return this.sql`SELECT * FROM aruaru_diff(${from}, ${to})`;
  }

  async merge(fromBranch: string) {
    return this.sql`SELECT aruaru_merge(${fromBranch})`;
  }

  // タイムトラベル
  async asOf(table: string, commitId: string) {
    return this.sql`SELECT * FROM ${this.sql(table)} AS OF COMMIT ${commitId}`;
  }
}

// ── 使用例 ───────────────────────────────────────────────────
async function main() {
  const db = new AruaruDB(sql);

  // ブランチ作成
  await db.branch("feature/add-products");

  // テーブル操作
  await sql`CREATE TABLE IF NOT EXISTS products (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    price NUMERIC(10,2)
  )`;
  await sql`INSERT INTO products (name, price) VALUES ('Widget', 9.99)`;

  // コミット
  const commitId = await db.commit("PHI", "Add products table with initial data");
  console.log("Committed:", commitId);

  // ログ
  const log = await db.log(5);
  log.forEach(row => console.log(`${row.short_id}  ${row.author}: ${row.message}`));

  await sql.end();
}

main().catch(console.error);
