# aruaru-db 公式 Go コネクタ / official Go connector

**独自の PostgreSQL ドライバではない。** 業界標準の
[`jackc/pgx/v5`](https://github.com/jackc/pgx) をそのまま使い、その上に
aruaru-db の Git-on-SQL 機能(`SELECT aruaru_commit('msg')` と
`... AS OF COMMIT '<id>'`)を Go の慣用 API で足しただけの薄い層。
`net/http` / chi / echo / gin どのフレームワークでも、pgx プールを渡せば
同じように使える。

正本: [`../../docs/CLIENTS.md`](../../docs/CLIENTS.md)

## 使い方

```go
db, err := aruaru.Connect(ctx, "postgres://app:secret@localhost:5433/app")
if err != nil { log.Fatal(err) }
defer db.Close()

db.Exec(ctx, "INSERT INTO items(id, qty) VALUES ('sword', 1)")
first, _ := db.Commit(ctx, "first import")

db.Exec(ctx, "UPDATE items SET qty = 5 WHERE id = 'sword'")
db.Commit(ctx, "restock")

// VersionlessAPI: 過去のコミット時点を読む
rows, _ := db.QueryAsOf(ctx, "SELECT qty FROM items WHERE id = $1", first, "sword")
```

`commit()` は結果列を列名(`aruaru_commit`、`AS alias` 不可)ではなく
先頭列の位置で読む。`QueryAsOf` の `commitID` は `IsSafeCommitID`
(英数字 + `-` `_`、≤128 文字)で検証してから文字列として `AS OF COMMIT`
へ埋め込む(aruaru-wire はこの句をバインドパラメータとして受け付けない
ため、SQL インジェクション対策として必須)。結果列は現状すべて
VARCHAR(text)で返るため、文字列として受けて parse すること。

## 検証状況

**未検証**——この開発環境に Go ツールチェーン(`go` コマンド)が存在
しないため、`go build`/`go test`/`go vet` のいずれも実行できていない。
`aruaru_test.go` にネットワーク不要なユニットテスト
(`IsSafeCommitID`)と、`ARUARU_DB_TEST_DSN` 設定時のみ走る実サーバ往復
テストを用意してある。Go 環境がある場所で

```bash
cd clients/go-aruaru-db
go build ./...
go test ./...
ARUARU_DB_TEST_DSN="postgres://app:secret@127.0.0.1:5433/app" go test -run Live ./...
```

を実行して確認すること。設計・API 形状は同一パターンの
[`rust-aruaru-db`](../rust-aruaru-db)(実サーバ往復検証済み、
2026-09-03)・[`node-aruaru-db`](../node-aruaru-db)(同)に倣っている。

---

# English

**NOT a custom PostgreSQL driver.** Uses the industry-standard
[`jackc/pgx/v5`](https://github.com/jackc/pgx) as-is and adds a thin,
idiomatic layer for aruaru-db's Git-on-SQL surface (`aruaru_commit` and
`AS OF COMMIT`). Works with any framework that accepts a pgx pool.

Source of truth: [`../../docs/CLIENTS.md`](../../docs/CLIENTS.md).

`Commit()` reads the result by column **position** (not name — aruaru-db
does not honor `AS alias` here). `QueryAsOf`'s `commitID` is validated
with `IsSafeCommitID` before being interpolated into the SQL text
(aruaru-wire has no bind-parameter support for `AS OF COMMIT`). Result
columns are all VARCHAR/text currently — scan as string and parse.

**Verification status: not verified in this session** — no `go` toolchain
is available in this development environment, so `go build`/`go test`
could not be run here. Please run them (see commands above) wherever a Go
toolchain is available. The design mirrors the already live-verified
[`rust-aruaru-db`](../rust-aruaru-db) and [`node-aruaru-db`](../node-aruaru-db)
connectors (2026-09-03).
