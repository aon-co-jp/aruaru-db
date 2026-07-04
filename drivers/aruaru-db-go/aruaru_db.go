// aruaru-DB Go ドライバー
//
// パッケージ名: github.com/aruaru-db/aruaru-db-go
// go get github.com/aruaru-db/aruaru-db-go
//
// 内部依存: github.com/jackc/pgx/v5
//
// aruaru-DB は PostgreSQL ワイヤ互換のため pgx がそのまま使えます。
// このドライバーは Git-on-SQL 操作を型付き API で包むラッパーです。
//
// 使用例:
//
//	import aruarudb "github.com/aruaru-db/aruaru-db-go"
//
//	ctx := context.Background()
//	db, err := aruarudb.Connect(ctx, "postgres://root@localhost:5432/aruaru")
//	if err != nil { log.Fatal(err) }
//	defer db.Close()
//
//	_ = db.Branch(ctx, "feature/go-test")
//	_, _ = db.Execute(ctx, "CREATE TABLE tasks (id INT, title TEXT)")
//	commitID, _ := db.Commit(ctx, "Add tasks via aruaru-db-go")
//	fmt.Println("Committed:", commitID)

package aruarudb

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// ── 型定義 ───────────────────────────────────────────────────

// Client は aruaru-DB への接続を表す。
// Go の慣例に従い、型名はシンプルに Client とし、
// パッケージ名 aruarudb で識別する (aruarudb.Client)。
type Client struct {
	pool *pgxpool.Pool
}

// LogEntry はコミットログの1エントリを表す。
type LogEntry struct {
	ID        string `db:"id"`
	ShortID   string `db:"short_id"`
	Author    string `db:"author"`
	Message   string `db:"message"`
	Timestamp string `db:"timestamp"`
	RootHash  string `db:"root_hash"`
}

// DiffStat は2ブランチ間の差分統計を表す。
type DiffStat struct {
	Added    int64
	Removed  int64
	Modified int64
}

// ── コンストラクタ ────────────────────────────────────────────

// Connect は DSN から接続プールを作成する。
//
//	dsn: "postgres://root@localhost:5432/aruaru"
func Connect(ctx context.Context, dsn string) (*Client, error) {
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		return nil, fmt.Errorf("aruaru-db-go connect: %w", err)
	}
	return &Client{pool: pool}, nil
}

// ── Git-on-SQL ────────────────────────────────────────────────

// Branch はブランチを作成する。
func (c *Client) Branch(ctx context.Context, name string) error {
	_, err := c.pool.Exec(ctx, "SELECT aruaru_branch($1)", name)
	return err
}

// Checkout はブランチを切り替える。
func (c *Client) Checkout(ctx context.Context, name string) error {
	_, err := c.pool.Exec(ctx, "SELECT aruaru_checkout($1)", name)
	return err
}

// CurrentBranch は現在のブランチ名を返す。
func (c *Client) CurrentBranch(ctx context.Context) (string, error) {
	var name string
	err := c.pool.QueryRow(ctx, "SELECT aruaru_current_branch()").Scan(&name)
	return name, err
}

// Commit はコミットしてコミット ID を返す。
func (c *Client) Commit(ctx context.Context, message string) (string, error) {
	var commitID string
	err := c.pool.QueryRow(ctx, "SELECT aruaru_commit($1)", message).Scan(&commitID)
	return commitID, err
}

// Merge は fast-forward マージしてコミット ID を返す。
func (c *Client) Merge(ctx context.Context, fromBranch string) (string, error) {
	var commitID string
	err := c.pool.QueryRow(ctx, "SELECT aruaru_merge($1)", fromBranch).Scan(&commitID)
	return commitID, err
}

// Log はコミットログを取得する。
func (c *Client) Log(ctx context.Context, limit int) ([]LogEntry, error) {
	rows, err := c.pool.Query(ctx,
		"SELECT id, short_id, author, message, timestamp FROM aruaru_log LIMIT $1", limit)
	if err != nil {
		return nil, err
	}
	return pgx.CollectRows(rows, pgx.RowToStructByName[LogEntry])
}

// Diff は2ブランチ間の差分統計を返す。
func (c *Client) Diff(ctx context.Context, from, to string) (DiffStat, error) {
	var s DiffStat
	err := c.pool.QueryRow(ctx,
		"SELECT added, removed, modified FROM aruaru_diff($1, $2)", from, to,
	).Scan(&s.Added, &s.Removed, &s.Modified)
	return s, err
}

// ── 汎用 SQL ─────────────────────────────────────────────────

// Execute は SQL を実行して変更行数を返す。
func (c *Client) Execute(ctx context.Context, sql string, args ...any) (int64, error) {
	tag, err := c.pool.Exec(ctx, sql, args...)
	return tag.RowsAffected(), err
}

// Query は SELECT を実行して行マップのスライスを返す。
func (c *Client) Query(ctx context.Context, sql string, args ...any) ([]map[string]any, error) {
	rows, err := c.pool.Query(ctx, sql, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	fields := rows.FieldDescriptions()
	var result []map[string]any
	for rows.Next() {
		vals, err := rows.Values()
		if err != nil {
			return nil, err
		}
		row := make(map[string]any, len(fields))
		for i, f := range fields {
			row[string(f.Name)] = vals[i]
		}
		result = append(result, row)
	}
	return result, rows.Err()
}

// Pool は生の pgxpool.Pool を返す (高度な用途向け)。
func (c *Client) Pool() *pgxpool.Pool { return c.pool }

// Close は接続プールを閉じる。
func (c *Client) Close() { c.pool.Close() }
