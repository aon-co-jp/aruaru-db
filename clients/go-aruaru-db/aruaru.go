// Package aruaru is the aruaru-db official Go connector (thin wrapper).
//
// これは独自の PostgreSQL ドライバではない。業界標準の非同期クライアント
// github.com/jackc/pgx/v5 をそのまま使い、その上に aruaru-db の Git-on-SQL
// 機能(SELECT aruaru_commit('msg') と ... AS OF COMMIT '<id>')を Go の
// 慣用 API で足しただけの薄い層。
//
// This is NOT a custom PostgreSQL driver. It uses the industry-standard
// client github.com/jackc/pgx/v5 as-is and only adds idiomatic helpers for
// aruaru-db's Git-on-SQL surface (aruaru_commit and AS OF COMMIT).
//
// 正本 / source of truth: ../../docs/CLIENTS.md
package aruaru

import (
	"context"
	"errors"
	"fmt"
	"regexp"
	"strings"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// ErrNoCommitID is returned when aruaru_commit() unexpectedly yields no row.
var ErrNoCommitID = errors.New("aruaru_commit() returned no commit id")

// safeCommitID matches aruaru-db commit ids (hex/UUID-ish: alnum, '-', '_').
var safeCommitID = regexp.MustCompile(`^[A-Za-z0-9_-]{1,128}$`)

// IsSafeCommitID reports whether id is safe to interpolate literally into
// `AS OF COMMIT '<id>'` (aruaru-wire does not support AS OF as a bind
// parameter, so this guards against SQL injection before any string
// concatenation happens).
func IsSafeCommitID(id string) bool {
	return safeCommitID.MatchString(id)
}

// InvalidCommitIDError is returned by QueryAsOf when the given commit id is
// not safe to interpolate.
type InvalidCommitIDError struct{ ID string }

func (e *InvalidCommitIDError) Error() string {
	return fmt.Sprintf("commit id %q is not a safe literal (expected hex / [A-Za-z0-9_-], <=128 chars)", e.ID)
}

// DB wraps a pgx connection pool with aruaru-db's Git-on-SQL helpers.
type DB struct {
	pool *pgxpool.Pool
}

// Connect opens a pgx pool against dsn (libpq or "postgres://" form).
func Connect(ctx context.Context, dsn string) (*DB, error) {
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		return nil, fmt.Errorf("aruaru-db connect failed: %w", err)
	}
	return &DB{pool: pool}, nil
}

// FromPool wraps an already-constructed pgx pool (bring your own TLS/pool config).
func FromPool(pool *pgxpool.Pool) *DB { return &DB{pool: pool} }

// Pool returns the underlying pgx pool for anything this wrapper doesn't cover.
func (d *DB) Pool() *pgxpool.Pool { return d.pool }

// Close closes the underlying pool.
func (d *DB) Close() { d.pool.Close() }

// Exec is a thin passthrough to pool.Exec.
func (d *DB) Exec(ctx context.Context, sql string, args ...any) error {
	_, err := d.pool.Exec(ctx, sql, args...)
	return err
}

// Query is a thin passthrough to pool.Query.
func (d *DB) Query(ctx context.Context, sql string, args ...any) (pgx.Rows, error) {
	return d.pool.Query(ctx, sql, args...)
}

// Commit snapshots the current state of all tables and returns the new
// commit id (SELECT aruaru_commit($1)).
//
// Note: aruaru-db does not honor `AS alias` on this function's result
// column — the column is literally named `aruaru_commit`. We read it by
// position (index 0), never by name.
func (d *DB) Commit(ctx context.Context, message string) (string, error) {
	row := d.pool.QueryRow(ctx, "SELECT aruaru_commit($1)", message)
	var commitID string
	if err := row.Scan(&commitID); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return "", ErrNoCommitID
		}
		return "", fmt.Errorf("aruaru-db commit query failed: %w", err)
	}
	if commitID == "" {
		return "", ErrNoCommitID
	}
	return commitID, nil
}

// QueryAsOf runs baseSelect (a plain SELECT with no AS OF COMMIT clause) as
// of the given historical commitID (VersionlessAPI time travel). commitID
// is validated with IsSafeCommitID before being interpolated into the SQL
// text, since AS OF COMMIT cannot be a bind parameter on aruaru-wire.
//
// Result columns come back as VARCHAR/text regardless of source column
// type — scan into string and parse, never assume a typed column.
func (d *DB) QueryAsOf(ctx context.Context, baseSelect, commitID string, args ...any) (pgx.Rows, error) {
	if !IsSafeCommitID(commitID) {
		return nil, &InvalidCommitIDError{ID: commitID}
	}
	sql := strings.TrimRight(baseSelect, " \t\n") + " AS OF COMMIT '" + commitID + "'"
	return d.pool.Query(ctx, sql, args...)
}
