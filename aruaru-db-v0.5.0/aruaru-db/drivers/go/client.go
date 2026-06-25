// aruaru-DB Go Client
// go get github.com/aruaru-db/aruaru-go
// 内部依存: github.com/jackc/pgx/v5

package aruaru

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

type Client struct {
	pool *pgxpool.Pool
}

func Connect(ctx context.Context, dsn string) (*Client, error) {
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		return nil, fmt.Errorf("aruaru connect: %w", err)
	}
	return &Client{pool: pool}, nil
}

func (c *Client) Branch(ctx context.Context, name string) error {
	_, err := c.pool.Exec(ctx, "SELECT aruaru_branch($1)", name)
	return err
}

func (c *Client) Commit(ctx context.Context, message string) (string, error) {
	var commitID string
	err := c.pool.QueryRow(ctx, "SELECT aruaru_commit($1)", message).Scan(&commitID)
	return commitID, err
}

type LogEntry struct {
	ID        string
	ShortID   string
	Author    string
	Message   string
	Timestamp string
}

func (c *Client) Log(ctx context.Context, limit int) ([]LogEntry, error) {
	rows, err := c.pool.Query(ctx,
		"SELECT id, short_id, author, message, timestamp FROM aruaru_log LIMIT $1", limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	return pgx.CollectRows(rows, pgx.RowToStructByName[LogEntry])
}

func (c *Client) Close() { c.pool.Close() }

// ── 使用例 ──────────────────────────────────────────────────
// func main() {
//     ctx := context.Background()
//     db, _ := aruaru.Connect(ctx, "postgres://root@localhost:5432/aruaru")
//     defer db.Close()
//     db.Branch(ctx, "feature/go-test")
//     commitID, _ := db.Commit(ctx, "Go client test")
//     fmt.Println("Committed:", commitID)
// }
