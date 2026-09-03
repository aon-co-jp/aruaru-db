package aruaru

import (
	"context"
	"os"
	"testing"

	"github.com/jackc/pgx/v5"
)

func TestIsSafeCommitID(t *testing.T) {
	safe := []string{
		"a1b2c3d4e5f6",
		"9f8e7d6c-1234-4abc-9def-000011112222",
		"commit_42-X",
	}
	for _, s := range safe {
		if !IsSafeCommitID(s) {
			t.Errorf("expected %q to be safe", s)
		}
	}

	unsafe := []string{
		"",
		"abc'; DROP TABLE items; --",
		"abc def",
	}
	for _, s := range unsafe {
		if IsSafeCommitID(s) {
			t.Errorf("expected %q to be rejected", s)
		}
	}

	long := make([]byte, 200)
	for i := range long {
		long[i] = 'x'
	}
	if IsSafeCommitID(string(long)) {
		t.Error("expected an overlong id to be rejected")
	}
}

func TestQueryAsOfRejectsUnsafeCommitIDBeforeTouchingTheNetwork(t *testing.T) {
	// No pool constructed at all: DB{} has a nil pool. If QueryAsOf reached
	// the network it would panic on the nil pool — proving the id was
	// rejected before any network I/O.
	d := &DB{}
	_, err := d.QueryAsOf(context.Background(), "SELECT qty FROM items", "' OR 1=1 --")
	if err == nil {
		t.Fatal("expected an InvalidCommitIDError")
	}
	var target *InvalidCommitIDError
	if _, ok := err.(*InvalidCommitIDError); !ok {
		t.Fatalf("expected *InvalidCommitIDError, got %T (%v)", err, target)
	}
}

// TestLiveCommitAndAsOfRoundTrip exercises a real aruaru-server. Only runs
// when ARUARU_DB_TEST_DSN is set (e.g.
// "postgres://app:secret@127.0.0.1:5433/app").
func TestLiveCommitAndAsOfRoundTrip(t *testing.T) {
	dsn := os.Getenv("ARUARU_DB_TEST_DSN")
	if dsn == "" {
		t.Skip("set ARUARU_DB_TEST_DSN to run against a live aruaru-server")
	}
	ctx := context.Background()
	db, err := Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer db.Close()

	must := func(err error) {
		t.Helper()
		if err != nil {
			t.Fatalf("exec failed: %v", err)
		}
	}
	must(db.Exec(ctx, "CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY, qty INT)"))
	must(db.Exec(ctx, "INSERT INTO items(id, qty) VALUES ('sword', 1) ON CONFLICT (id) DO UPDATE SET qty = EXCLUDED.qty"))

	first, err := db.Commit(ctx, "first import")
	if err != nil {
		t.Fatalf("commit: %v", err)
	}
	must(db.Exec(ctx, "UPDATE items SET qty = 5 WHERE id = 'sword'"))
	if _, err := db.Commit(ctx, "restock"); err != nil {
		t.Fatalf("commit 2: %v", err)
	}

	// aruaru-wire returns ordinary table columns as VARCHAR(text); scan as
	// string and parse.
	rows, err := db.Query(ctx, "SELECT qty FROM items WHERE id = 'sword'")
	if err != nil {
		t.Fatalf("query: %v", err)
	}
	var latest string
	if !rows.Next() {
		t.Fatal("expected a row")
	}
	if err := rows.Scan(&latest); err != nil {
		t.Fatalf("scan: %v", err)
	}
	rows.Close()
	if latest != "5" {
		t.Fatalf("expected latest qty=5, got %q", latest)
	}

	oldRows, err := db.QueryAsOf(ctx, "SELECT qty FROM items WHERE id = 'sword'", first)
	if err != nil {
		t.Fatalf("query_as_of: %v", err)
	}
	defer oldRows.Close()
	if !oldRows.Next() {
		t.Fatal("expected a historical row")
	}
	var old string
	if err := oldRows.Scan(&old); err != nil {
		t.Fatalf("scan: %v", err)
	}
	if old != "1" {
		t.Fatalf("AS OF COMMIT must return the historical value, got %q", old)
	}
	_ = pgx.ErrNoRows // keep import used even if error path above changes
}
