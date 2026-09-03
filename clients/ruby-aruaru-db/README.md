# `aruaru-db` gem — 公式 Ruby コネクタ(薄いラッパー)

**独自の PostgreSQL ドライバではない。** 業界標準の `pg` gem(libpq の
バインディング)をそのまま使い、その上に aruaru-db の Git-on-SQL 機能を
Ruby の慣用 API で足すだけの薄い層。Rails(`adapter: postgresql`)からも
素の `PG::Connection` からも使える。

正本: [`../../docs/CLIENTS.md`](../../docs/CLIENTS.md)

## インストール

```bash
gem install aruaru-db
# Gemfile: gem "aruaru-db"
```

## API

| メソッド | 実体 |
|---|---|
| `Aruaru::Db::Client.connect(dsn)` / `.from_pg(pg_connection)` | `PG::Connection` をそのまま |
| `#execute(sql, params)` | `PG::Connection#exec_params` の薄い透過 |
| `#commit(message) -> String` | `SELECT aruaru_commit($1)` → commit_id(**結果列は位置〈先頭〉で読む。`AS alias` は効かない**) |
| `#query_as_of(base_select, commit_id, params)` | 末尾へ ` AS OF COMMIT '<id>'` を安全に付与。`Aruaru::Db.safe_commit_id?`(英数字+`-`/`_`、≤128)で検証、非安全なら `Aruaru::Db::InvalidCommitId`(SQL インジェクション防止、ネットワークに触れる前に拒否) |
| `#conn` | 内部の `PG::Connection`(透過的に何でも) |

## 例(素の `pg`)

```ruby
require "aruaru/db"

db = Aruaru::Db::Client.connect("host=localhost port=5433 user=app password=secret dbname=app")
db.execute("INSERT INTO items(id, qty) VALUES ('sword', 1)")
first = db.commit("first import")

db.execute("UPDATE items SET qty = 5 WHERE id = 'sword'")
db.commit("restock")

# VersionlessAPI: 過去のコミット時点を読む(最新は 5、これは 1)
old = db.query_as_of("SELECT qty FROM items WHERE id = 'sword'", first)
old.first["qty"] # => "1" (aruaru-wire は結果列を VARCHAR/text で返す)
```

## Rails

`config/database.yml` は標準の `adapter: postgresql`(`port: 5433`)。
ActiveRecord の通常 CRUD はそのまま。Git-on-SQL だけこのラッパーで、
ActiveRecord の生コネクションを再利用する:

```ruby
db = Aruaru::Db::Client.from_pg(ActiveRecord::Base.connection.raw_connection)
commit = db.commit("after migration")
```

## ビルド・テスト

```bash
bundle install
bundle exec rspec           # ネットワーク不要ぶん
ARUARU_DB_TEST_DSN="host=127.0.0.1 port=5433 user=app password=secret dbname=app" \
  bundle exec rspec -e "live round trip"
gem build aruaru-db.gemspec
```

## 検証状況

**未検証**(この開発環境に `ruby`/`gem` コマンドが導入されていないため
`bundle exec rspec`/`gem build` を実行できなかった——`ruby -v`/`gem -v`
がコマンド未検出)。コードは `rust-aruaru-db`/`node-aruaru-db`(実サーバ
往復まで 2026-09-03 に green 確認済み)/`php-aruaru-db` と同じ設計
(commit_id 検証 → 位置ベースの `aruaru_commit` 列読み取り →
`AS OF COMMIT` の安全な文字列連結)を踏襲しており、目視レビュー・
`double`(モック)ベースのユニットテストの記述は行ったが、実際の
`rspec` 実行・`gem build`・実サーバ往復はこのセッションでは検証できて
いない。Ruby ツールチェーンが使える環境で上記コマンドを実行して確認
すること。
