import type { Pool, Client, QueryResult, QueryResultRow } from 'pg';

export function isSafeCommitId(id: unknown): id is string;
export class InvalidCommitId extends Error {}

export class AruaruDb {
  constructor(poolOrClient: Pool | Client);
  static connect(config: string | object): Promise<AruaruDb>;
  static fromPool(poolOrClient: Pool | Client): AruaruDb;
  get raw(): Pool | Client;
  query<R extends QueryResultRow = any>(sql: string, params?: any[]): Promise<QueryResult<R>>;
  /** Git-on-SQL: snapshot all tables, return the commit id. */
  commit(message: string): Promise<string>;
  /** VersionlessAPI: run `baseSelect` (no AS OF clause) as of a past commit. */
  queryAsOf<R extends QueryResultRow = any>(
    baseSelect: string,
    commitId: string,
    params?: any[],
  ): Promise<QueryResult<R>>;
}
