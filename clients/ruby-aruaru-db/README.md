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

**2026-09-05: rspec実行は実際に検証済み**——RubyInstaller公式exe
(3.3.6)をこの開発機へ実際にネットから導入し、`gem install rspec`
(純Rubyのため`pg`のネイティブ拡張ビルド不要)→`rspec`を実行:

```
.....

Finished in 0.06589 seconds
5 examples, 0 failures
```

`lib/aruaru/db.rb`は`require "pg"`を`.connect`メソッド内でのみ遅延
実行するため、`double`(モック)ベースの単体テスト(本 gem の主要な
検証対象)は`pg` gem自体が未導入でも実行できることを確認した。

**2026-09-06追記: `pg` gem導入+実サーバ往復も検証済み**——
`gem install pg`を実行したところ、Windows用の**プリコンパイル済み
バイナリ**(`pg-1.6.3-x64-mingw-ucrt`)が取得され、Cコンパイラ環境
無しでインストールできた(当初想定していた「追加のCコンパイラ環境が
必要」という懸念は誤りだった)。ローカルに実際に`aruaru-server`を
起動し、`ARUARU_DB_TEST_DSN="host=127.0.0.1 port=5433 user=app
password=secret dbname=app"`を設定した上で`rspec`を再実行:

```
......

6 examples, 0 failures
```

(既存5件+live round trip 1件、commit → `AS OF COMMIT`往復が実際に
成功することを確認した)。設計は `rust-aruaru-db`/`node-aruaru-db`(実サーバ往復まで
2026-09-03 に green 確認済み)/`php-aruaru-db` と同じ
(commit_id 検証 → 位置ベースの `aruaru_commit` 列読み取り →
`AS OF COMMIT` の安全な文字列連結)を踏襲している。
