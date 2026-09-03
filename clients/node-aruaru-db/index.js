'use strict';
// @aruaru/db — aruaru-db 公式 Node コネクタ(薄いラッパー)
//
// 独自の PostgreSQL ドライバではない。標準の `pg`(node-postgres)を
// そのまま使い、その上に Git-on-SQL を Node の慣用 API で足すだけ。
// Express / Fastify / NestJS など、どの FW でも同じ。
//
// 正本: ../../docs/CLIENTS.md

const SAFE_COMMIT_ID = /^[A-Za-z0-9_-]{1,128}$/;

/** commit_id が `AS OF COMMIT '<id>'` のリテラルとして安全か。 */
function isSafeCommitId(id) {
  return typeof id === 'string' && SAFE_COMMIT_ID.test(id);
}

class InvalidCommitId extends Error {
  constructor(id) {
    super(`commit id ${JSON.stringify(id)} is not a safe literal (expected hex / [A-Za-z0-9_-], <=128 chars)`);
    this.name = 'InvalidCommitId';
  }
}

function asOfSql(baseSelect, commitId) {
  if (!isSafeCommitId(commitId)) throw new InvalidCommitId(commitId);
  return `${String(baseSelect).replace(/\s+$/, '')} AS OF COMMIT '${commitId}'`;
}

class AruaruDb {
  /**
   * @param {import('pg').Pool|import('pg').Client} poolOrClient  既存の pg Pool / Client
   */
  constructor(poolOrClient) {
    this._c = poolOrClient;
  }

  /**
   * @param {string|object} config  pg の接続文字列 or config オブジェクト
   * @returns {Promise<AruaruDb>}
   */
  static async connect(config) {
    const pg = require('pg'); // lazy: driver は connect 時だけ必要
    const pool = new pg.Pool(typeof config === 'string' ? { connectionString: config } : config);
    // 起動時に 1 回だけ疎通確認
    const c = await pool.connect();
    c.release();
    return new AruaruDb(pool);
  }

  /** 既存の pg Pool / Client を包む。 */
  static fromPool(poolOrClient) {
    return new AruaruDb(poolOrClient);
  }

  /** 内部の pg Pool / Client。透過的に何でも。 */
  get raw() {
    return this._c;
  }

  /** pg の query をそのまま。 */
  query(sql, params) {
    return this._c.query(sql, params);
  }

  /** Git-on-SQL: 全テーブルをスナップショットし commit_id を返す。 */
  async commit(message) {
    // aruaru-db は結果列を関数名(`aruaru_commit`)で返し、`AS alias` は
    // 効かない。列名に依存せず先頭列の値を取る。
    const r = await this._c.query('SELECT aruaru_commit($1)', [message]);
    const first = r.rows.length ? Object.values(r.rows[0])[0] : null;
    if (first == null) {
      throw new Error('aruaru_commit() returned no commit id');
    }
    return String(first);
  }

  /**
   * VersionlessAPI: `baseSelect` を過去のコミット時点で読む。
   * `baseSelect` は `AS OF COMMIT` を含まない通常の SELECT。
   * @returns {Promise<import('pg').QueryResult>}
   */
  queryAsOf(baseSelect, commitId, params) {
    return this._c.query(asOfSql(baseSelect, commitId), params);
  }
}

module.exports = { AruaruDb, isSafeCommitId, InvalidCommitId, _asOfSql: asOfSql };
