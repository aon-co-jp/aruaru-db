# aruaru-db クライアント接続ガイド / Client Connection Guide

**正本 (source of truth)**. 対応言語・フレームワーク・OS からの接続方法を
まとめる。日本語 → English は下段。

---

## 0. 結論を先に:「独自ドライバ」も「device driver installer」も要らない

aruaru-db は 2 つの**標準契約**だけを公開する。どちらも普通の TCP。

| 契約 | 既定ポート | 何を話すか |
|---|---|---|
| **PostgreSQL ワイヤプロトコル (pgwire)** | `5433` | PostgreSQL 互換。`SELECT` / `INSERT` / トランザクション / prepared statement(拡張プロトコル)/ Git-on-SQL 関数(`aruaru_commit` 等)/ `SELECT ... AS OF COMMIT '<id>'` |
| **GraphQL over HTTP** | `4001`(`/graphql`) | VersionlessAPI / Federation / 観測クエリ / 管理系(`x-admin-token`) |

**したがって:**

- Java・Rust・Python・PHP・Go・Node・.NET・Ruby…**どの言語でも、その
  エコシステムに既にある PostgreSQL クライアント**(JDBC / psycopg /
  asyncpg / PDO_pgsql / pgx / node-postgres / Npgsql / ruby-pg …)で
  **そのまま接続できる**。aruaru-db 側が用意する「言語別ドライバ」は
  無い(必要ない)。
- `<repo>-<言語>-<OS>-device-driver-installer.exe` のような成果物は
  **作らない**。理由:(a) これらは「デバイスドライバ」ではなくネット
  ワークサービスのクライアントであり、(b) 各言語の標準 PostgreSQL
  ドライバは何年もセキュリティ監査を受けた成熟実装で、それを 5 言語 ×
  8 OS ぶん再実装するのは `CLAUDE.md` の「闇雲な代替を避ける」原則に
  真っ向から反する。
- 代わりに **`clients/<スタック>/` にそのまま動くサンプル**(コネクタ
  キット)を置く。命名は `clients/python-fastapi/` のように
  `<言語>-<フレームワーク>`。実行手順は各 `clients/*/README.md`。

「installer 風の名前が要る」場合の対応: コネクタキットの配布物名は
`aruaru-db-<言語>-connector`(例 `aruaru-db-python-connector`)とし、
`.exe` の device-driver installer は用意しない(上記理由)。誤記
(`aruaru-db-python-windows-device-driver-installer.exe` 等)を見つけたら
この `aruaru-db-<言語>-connector` へ寄せる。

### 0.2 公式「薄いコネクタ」パッケージ(標準ドライバ + Git-on-SQL ヘルパー)

「実体のあるコネクタ層が欲しい」向けに、**各言語の標準ドライバをそのまま
使い、その上に `commit()` / `query_as_of()` を慣用 API で足しただけ**の
薄いパッケージを `clients/` に置く。ドライバの再実装ではない(速度差なし)。
commit_id は全実装で「英数字 + `-` `_`、≤128 文字」を検証し、非安全なら
ネットワークに触れる前にエラー(SQL インジェクション防止)。

| パッケージ | 言語 | 下敷きの標準ドライバ | 同期/非同期 | ディレクトリ | テスト(ネットワーク不要ぶん) |
|---|---|---|---|---|---|
| `aruaru-db-connector` | Rust | `tokio-postgres` | 非同期 | `clients/rust-aruaru-db/` | ✅ green(2 + doctest) |
| `aruaru-db` (PyPI) | Python | `asyncpg`(async)/ `psycopg` v3(sync) | 両方(`AruaruDb` / `AruaruDbSync`) | `clients/python-aruaru-db/` | ✅ green(4) |
| `@aruaru/db` (npm) | Node/TS | `pg`(node-postgres) | 非同期 | `clients/node-aruaru-db/` | ✅ green(4) |
| `aruaru/db` (Composer) | PHP | `PDO`(pdo_pgsql) | 同期 | `clients/php-aruaru-db/` | 未検証(この環境に PHP なし) |

素の接続レシピ(パッケージを使わず標準ドライバだけ)は §3。COBOL は
埋め込み SQL(`EXEC SQL`)で `clients/cobol/`。

### 0.1 「同期/非同期/ハイブリッドを、言語・OS が違っても速度を犠牲にせず互換」— それは pgwire が既に達成している

- **ワイヤ上のバイト列は同期でも非同期でも同一**。「同期 or 非同期」は
  クライアントライブラリの I/O モデル(スレッドブロッキング vs
  イベントループ)の違いにすぎず、PostgreSQL ワイヤプロトコル自体は
  どちらにも同じフレームを流す。→ 同じ aruaru-db に、JDBC(同期)と
  asyncpg(非同期)と Npgsql(両対応=ハイブリッド)が**同時に**繋がって
  よい。
- **速度の犠牲がない理由**: 各言語が使うのは、その言語で何年も最適化
  ・監査されてきた**標準ドライバそのもの**。aruaru-db 独自の変換層を
  挟まない = 追加のオーバーヘッドがゼロ。もし独自「デバイスドライバ」
  層を作れば、それはむしろ 1 段オーバーヘッドを**増やす**。
- **ハイブリッド**が必要なアプリ(同じプロセス内で同期経路と非同期経路
  を併用)は、ハイブリッド対応の標準ドライバ(Npgsql、`pgx`、JDBC+
  仮想スレッド、psycopg3 の sync/async 両 API)を選べばよい。aruaru-db
  側は何もしない(=互換が保たれる)。
- **OS 差**: pgwire は TCP(+TLS)だけに依存する。Windows/macOS/Linux/
  UNIX/メインフレーム/Android/iOS のいずれでも、その OS で動く標準
  PostgreSQL ドライバがそのまま使える(§4)。
- **フレームワーク差**: フレームワークは「どのドライバをどう DI するか」
  の違いでしかない。Spring/Quarkus/FastAPI/Django/Laravel/Rails/
  ASP.NET/Axum/Poem/RPoem いずれも、上のドライバをそのまま受け取る
  (§3 に各フレームワークの設定行)。

**結論**: 「速度をほぼ犠牲にせず、言語・OS・同期/非同期・フレームワークを
またいで互換」という要件は、**新しい成果物を作らないこと**で最もよく
満たされる。このガイド(+ `clients/` の実行可能サンプル)がその成果物。

---

## 1. 早見表(言語 × フレームワーク × 同期/非同期)

| 言語 | 同期ドライバ | 非同期ドライバ | フレームワーク例 | サンプル |
|---|---|---|---|---|
| **Java** | JDBC (`org.postgresql:postgresql`) | R2DBC (`io.r2dbc:r2dbc-postgresql`) | Spring Boot / Quarkus / 素の JDBC | `clients/java-jdbc/` |
| **Rust** | `postgres` (crate) | `tokio-postgres` / `sqlx` | Axum / Poem / RPoem | `clients/rust-axum/` |
| **Python** | `psycopg` (v3, `psycopg[binary]`) | `asyncpg` / `psycopg` async | FastAPI / Django / Flask+SQLAlchemy | `clients/python-fastapi/` |
| **PHP** | `PDO_pgsql` / `pgsql` 拡張 | (Swoole/AMPHP 経由) | Laravel (`pgsql` ドライバ) / Symfony | `clients/php-laravel/` |
| **Go** | `database/sql` + `pgx` stdlib | `pgx` / `pgxpool` | net/http / Gin / Echo | `clients/go-pgx/` |
| **Node / TS** | (なし。JS は基本非同期) | `pg` / `postgres.js` | Express / Fastify / NestJS | `clients/node-pg/` |
| **.NET (C#)** | `Npgsql`(同期 API) | `Npgsql`(async API)/ EF Core | ASP.NET Core / Minimal API | `clients/dotnet-npgsql/` |
| **Ruby** | `pg` gem | (async gem 経由) | Rails (`adapter: postgresql`) | — (下記レシピ参照) |
| **COBOL** | 埋め込み SQL(`EXEC SQL`)経由の同期 | — | Micro Focus / GnuCOBOL(OCESQL / ODBC / libpq) | `clients/cobol/` |

> **GraphQL** はどの言語でも「HTTP で `POST /graphql`」なので専用ドライバ
> 不要(`reqwest` / `httpx` / `OkHttp` / `fetch` / `HttpClient` 等)。

---

## 2. 接続文字列

- **pgwire (libpq 形式)**: `postgresql://<user>:<pass>@<host>:5433/<db>?sslmode=require`
- **pgwire (JDBC 形式)**: `jdbc:postgresql://<host>:5433/<db>?ssl=true`
- **pgwire (R2DBC 形式)**: `r2dbc:postgresql://<user>:<pass>@<host>:5433/<db>`
- **GraphQL**: `http(s)://<host>:4001/graphql`(管理系は `x-admin-token` ヘッダ)

`<user>`/`<pass>` は `ARUARU_USERS` で設定した SCRAM 認証情報。TLS は
`aruaru-server --tls-cert --tls-key` で終端(またはリバースプロキシ)。

---

## 3. レシピ(接続 → SELECT → `aruaru_commit` → `AS OF COMMIT`)

Git-on-SQL の 2 機能は**ただの SQL** なので全言語共通:

```sql
-- 現在値を書く
INSERT INTO items (id, qty) VALUES ('sword', 1);
-- コミット(commit_id が返る)
SELECT aruaru_commit('first import');
-- 値を更新して再コミット
UPDATE items SET qty = 5 WHERE id = 'sword';
SELECT aruaru_commit('restock');
-- 過去のコミット時点を読む(最新は qty=5、この行は qty=1 を返す)
SELECT qty FROM items WHERE id = 'sword' AS OF COMMIT '<first commit_id>';
```

### 3.1 Java

**同期 (JDBC)** — `build.gradle`: `implementation 'org.postgresql:postgresql:42.7.4'`
```java
try (var c = java.sql.DriverManager.getConnection(
        "jdbc:postgresql://localhost:5433/app?ssl=true", "app", "secret")) {
    try (var st = c.createStatement()) {
        st.execute("INSERT INTO items(id, qty) VALUES ('sword', 1)");
        var rs = st.executeQuery("SELECT aruaru_commit('first import')");
        rs.next(); String commit = rs.getString(1);
        st.execute("UPDATE items SET qty = 5 WHERE id = 'sword'");
        st.execute("SELECT aruaru_commit('restock')");
        var old = c.prepareStatement(
            "SELECT qty FROM items WHERE id = ? AS OF COMMIT ?");
        old.setString(1, "sword"); old.setString(2, commit);
        var r = old.executeQuery(); r.next();
        System.out.println("as-of: " + r.getInt(1)); // 1
    }
}
```
**非同期 (R2DBC)** — `io.r2dbc:r2dbc-postgresql`。`ConnectionFactories.get("r2dbc:postgresql://app:secret@localhost:5433/app")`
→ `Mono`/`Flux` で同じ SQL。**Spring Boot**: `spring.datasource.url=jdbc:postgresql://localhost:5433/app`
(同期) または `spring.r2dbc.url=r2dbc:postgresql://...`(WebFlux)。
**Quarkus**: `quarkus.datasource.jdbc.url=jdbc:postgresql://localhost:5433/app`。

### 3.2 Rust

**非同期 (tokio-postgres / sqlx)** — `Cargo.toml`:
`tokio-postgres = "0.7"` または `sqlx = { version = "0.8", features = ["postgres","runtime-tokio"] }`
```rust
let (client, conn) = tokio_postgres::connect(
    "host=localhost port=5433 user=app password=secret dbname=app sslmode=require",
    tokio_postgres::NoTls).await?;
tokio::spawn(conn);
client.execute("INSERT INTO items(id, qty) VALUES ('sword', 1)", &[]).await?;
let row = client.query_one("SELECT aruaru_commit('first import')", &[]).await?;
let commit: String = row.get(0);
client.execute("UPDATE items SET qty = 5 WHERE id = 'sword'", &[]).await?;
client.execute("SELECT aruaru_commit('restock')", &[]).await?;
let old = client.query_one(
    "SELECT qty FROM items WHERE id = $1 AS OF COMMIT $2", &[&"sword", &commit]).await?;
assert_eq!(old.get::<_, i32>(0), 1);
```
**同期** — `postgres = "0.19"`(`Client::connect(...)`、同じ SQL)。
**Axum / Poem / RPoem**: ハンドラ内で上記 `client`(`Arc` で共有、
`sqlx::PgPool` 推奨)を使うだけ。`clients/rust-axum/` に完全例。

### 3.3 Python

**非同期 (asyncpg)** — `pip install asyncpg`
```python
import asyncpg, asyncio
async def main():
    c = await asyncpg.connect("postgresql://app:secret@localhost:5433/app?ssl=require")
    await c.execute("INSERT INTO items(id, qty) VALUES ('sword', 1)")
    commit = await c.fetchval("SELECT aruaru_commit('first import')")
    await c.execute("UPDATE items SET qty = 5 WHERE id = 'sword'")
    await c.execute("SELECT aruaru_commit('restock')")
    old = await c.fetchval(
        "SELECT qty FROM items WHERE id = $1 AS OF COMMIT $2", "sword", commit)
    assert old == 1
    await c.close()
asyncio.run(main())
```
**同期 (psycopg 3)** — `pip install "psycopg[binary]"`:
`with psycopg.connect("host=localhost port=5433 ...") as c: c.execute(...)`。
**FastAPI**: 起動時に `asyncpg.create_pool(...)` を `app.state.pool` へ、
エンドポイントで `async with pool.acquire() as c:`。`clients/python-fastapi/` に完全例。
**Django**: `DATABASES['default'] = {'ENGINE':'django.db.backends.postgresql',
'HOST':'localhost','PORT':'5433', ...}`。**Flask + SQLAlchemy**:
`create_engine("postgresql+psycopg://app:secret@localhost:5433/app")`。

### 3.4 PHP

**同期 (PDO_pgsql)** — `php.ini` で `extension=pdo_pgsql`
```php
$c = new PDO('pgsql:host=localhost;port=5433;dbname=app;sslmode=require', 'app', 'secret');
$c->exec("INSERT INTO items(id, qty) VALUES ('sword', 1)");
$commit = $c->query("SELECT aruaru_commit('first import')")->fetchColumn();
$c->exec("UPDATE items SET qty = 5 WHERE id = 'sword'");
$c->exec("SELECT aruaru_commit('restock')");
$st = $c->prepare("SELECT qty FROM items WHERE id = ? AS OF COMMIT ?");
$st->execute(['sword', $commit]);
echo $st->fetchColumn(); // 1
```
**Laravel** — `config/database.php` の `pgsql` 接続をそのまま使う:
```php
// .env
DB_CONNECTION=pgsql
DB_HOST=127.0.0.1
DB_PORT=5433
DB_DATABASE=app
DB_USERNAME=app
DB_PASSWORD=secret
```
```php
$commit = DB::selectOne("SELECT aruaru_commit(?) AS c", ['first import'])->c;
$old = DB::selectOne(
  "SELECT qty FROM items WHERE id = ? AS OF COMMIT ?", ['sword', $commit])->qty;
```
Eloquent の通常 CRUD も動く(`AS OF COMMIT` は生 SQL / `DB::select`)。
`clients/php-laravel/` にマイグレーション + コントローラ例。

### 3.5 Go

**pgx** — `go get github.com/jackc/pgx/v5`
```go
c, _ := pgx.Connect(ctx, "postgres://app:secret@localhost:5433/app?sslmode=require")
c.Exec(ctx, "INSERT INTO items(id, qty) VALUES ('sword', 1)")
var commit string
c.QueryRow(ctx, "SELECT aruaru_commit('first import')").Scan(&commit)
c.Exec(ctx, "UPDATE items SET qty = 5 WHERE id = 'sword'")
c.Exec(ctx, "SELECT aruaru_commit('restock')")
var old int
c.QueryRow(ctx, "SELECT qty FROM items WHERE id=$1 AS OF COMMIT $2", "sword", commit).Scan(&old)
```
同期 `database/sql`: `sql.Open("pgx/v5", "postgres://...")`。プール: `pgxpool`。

### 3.6 Node / TypeScript

**pg** — `npm i pg`
```js
import pg from 'pg';
const c = new pg.Client({ connectionString: 'postgres://app:secret@localhost:5433/app', ssl: true });
await c.connect();
await c.query("INSERT INTO items(id, qty) VALUES ('sword', 1)");
const { rows } = await c.query("SELECT aruaru_commit('first import')");
const commit = rows[0].aruaru_commit;
await c.query("UPDATE items SET qty = 5 WHERE id = 'sword'");
await c.query("SELECT aruaru_commit('restock')");
const r = await c.query("SELECT qty FROM items WHERE id=$1 AS OF COMMIT $2", ['sword', commit]);
// r.rows[0].qty === 1
```
`postgres.js` も同様。Express/Fastify/NestJS は上記 `Client`/`Pool` を DI するだけ。

### 3.7 .NET (C#)

**Npgsql** — `dotnet add package Npgsql`
```csharp
await using var c = new NpgsqlConnection("Host=localhost;Port=5433;Database=app;Username=app;Password=secret;SSL Mode=Require");
await c.OpenAsync();
await using (var cmd = new NpgsqlCommand("INSERT INTO items(id,qty) VALUES ('sword',1)", c))
    await cmd.ExecuteNonQueryAsync();
var commit = (string)(await new NpgsqlCommand("SELECT aruaru_commit('first import')", c).ExecuteScalarAsync())!;
await new NpgsqlCommand("UPDATE items SET qty=5 WHERE id='sword'", c).ExecuteNonQueryAsync();
await new NpgsqlCommand("SELECT aruaru_commit('restock')", c).ExecuteNonQueryAsync();
var q = new NpgsqlCommand("SELECT qty FROM items WHERE id=@id AS OF COMMIT @c", c);
q.Parameters.AddWithValue("id", "sword"); q.Parameters.AddWithValue("c", commit);
var old = (int)(await q.ExecuteScalarAsync())!; // 1
```
同期 API(`Open()`/`ExecuteScalar()`)も同一。EF Core: `UseNpgsql("Host=...;Port=5433;...")`。

### 3.8 Ruby

**pg** — `gem 'pg'`。`PG.connect(host:'localhost', port:5433, dbname:'app', user:'app', password:'secret')`
→ `conn.exec` / `conn.exec_params`。**Rails**: `config/database.yml` の
`adapter: postgresql` / `port: 5433`。`AS OF COMMIT` は
`ActiveRecord::Base.connection.select_value(...)`。

### 3.8b RPoem / Poem(Rust の Web フレームワーク)から使う

**「RPoem 用ドライバ」「Poem 用ドライバ」は存在しないし、要らない。**
RPoem(`open-runo-poem-compat`)は tokio/hyper 上の HTTP 層であって、DB
クライアントを指定しない。RPoem アプリから aruaru-db へは、上の **Rust の
非同期 PostgreSQL クライアント**をそのまま使う:

| 選択肢 | いつ使うか |
|---|---|
| **`aruaru-db-connector`**(`clients/rust-aruaru-db/`) | `tokio-postgres` + `commit()` / `query_as_of()` の薄いラッパー。Git-on-SQL を慣用 API で使いたいとき |
| **`tokio-postgres` 生** | 依存を最小にしたいとき。`SELECT aruaru_commit($1)` / `... AS OF COMMIT $2` を自分で書く |
| **`sqlx`(`postgres`,`runtime-tokio`)** | コンパイル時クエリ検査 / `PgPool` が欲しいとき |
| **`open-runo-db::aruaru::AruaruDbBackend`**(RPoem 同梱) | put/get/delete/list の**汎用 KV** だけでよいとき。**Git-on-SQL(`aruaru_commit`/`AS OF COMMIT`)は未対応**なので、版管理が要るなら上の 3 つ |

RPoem には `Data<T>` 抽出子が無いため、共有状態(`Arc<AruaruDb>` /
`Arc<PgPool>`)はハンドラ登録クロージャで `Arc::clone` をキャプチャする
(`aruaru-llm` の RPoem 移行 HANDOFF 2026-07-31 と同じ形)。

```rust
let db = std::sync::Arc::new(aruaru_db_connector::AruaruDb::connect(&dsn).await?);
let db_h = db.clone();
route.at("/items/:id", post(handler_fn(move |req, p| {
    let db = db_h.clone();
    Box::pin(async move { /* db.commit(...).await / db.query_as_of(...).await */ })
})));
```

**互換性**: aruaru-db の pgwire は拡張プロトコル込みで PostgreSQL 互換
(`describe_portal` は 2026-07-14 修正済み)なので `tokio-postgres` /
`sqlx` はそのまま動く。**新規開発は不要**——`aruaru-db-connector` は
「あると便利」な薄い層であって必須ではない。

### 3.8c COBOL(埋め込み SQL `EXEC SQL`、独自ドライバなし)

aruaru-db は pgwire なので COBOL からは標準の 3 経路で繋がる:
(1) **ODBC**(psqlODBC を DSN 登録 + Micro Focus / OpenCOBOL ESQL でプリ
コンパイル、Windows / Linux / UNIX / **z/OS USS**)、(2) **libpq 直呼び**
(GnuCOBOL の `CALL "PQexec"` 等)、(3) **OCESQL**(`ocesql` で `EXEC SQL`
→ libpq)。ポートは 5433。Git-on-SQL はホスト変数へバインドするだけ:

```cobol
       EXEC SQL SELECT aruaru_commit(:H-MSG) INTO :H-COMMIT-ID END-EXEC.
       EXEC SQL
           SELECT qty INTO :H-QTY
           FROM items WHERE id = :H-ID
           AS OF COMMIT :H-COMMIT-ID
       END-EXEC.
```

完全な参照実装は `clients/cobol/ARUARU.cob`(プリコンパイル手順は
`clients/cobol/README.md`)。

### 3.9 GraphQL(全言語共通・HTTP のみ)

```bash
curl -s http://localhost:4001/graphql -H 'content-type: application/json' \
  -d '{"query":"query { hlcNow { wallNanos ordinal } }"}'
```
管理系は `-H "x-admin-token: <TOKEN>"`。言語側は普通の HTTP クライアント。

---

## 4. OS 別ドライバ入手表(独自インストーラは無し)

| OS / プラットフォーム | 標準 PostgreSQL クライアントの入手 |
|---|---|
| **Windows** | JDBC は jar 単体 / `pip install asyncpg` / `Install-Package Npgsql` / `npm i pg` / PHP は `php_pdo_pgsql.dll`(同梱) |
| **macOS** | 上記と同じ。`brew install libpq`(psql/PDO 用) |
| **Linux** | 各ディストロの `libpq5` / `postgresql-client`。言語パッケージは各エコシステムの標準 |
| **UNIX(Solaris / AIX / *BSD)** | JDBC(純 Java、そのまま動く)/ pkg の `postgresql-client` / `libpq` |
| **メインフレーム(IBM z/OS)** | **JDBC ドライバ(純 Java Type 4)** が z/OS 上の JVM でそのまま動く。ネイティブ言語からは USS の `libpq` |
| **Android(スマホ / タブレット)** | JDBC(`org.postgresql:postgresql`)を依存に追加 / Ktor + `r2dbc-postgresql` / OkHttp で GraphQL。**注意**: 端末から直接 DB に繋ぐ構成はネットワーク/認証設計を伴う(通常はアプリサーバ経由) |
| **iOS / iPadOS** | Swift: [`PostgresClientKit`](https://github.com/codewinsdotcom/PostgresClientKit) / [`PostgresNIO`](https://github.com/vapor/postgres-nio)(SwiftNIO ベース、非同期)。GraphQL は `URLSession` |
| **COBOL(全 OS + メインフレーム)** | psqlODBC(unixODBC / Windows ODBC / z/OS USS)+ `EXEC SQL`、または libpq を `CALL`、または OCESQL。`clients/cobol/` 参照 |

**共通の前提**: TCP と(推奨で)TLS が張れれば繋がる。ワイヤ形式が
PostgreSQL 互換なので、上記いずれも aruaru-db 専用のビルドは不要。

---

## 5. 拡張プロトコル(prepared statement)対応状況

多くの ORM / ドライバは既定で**拡張プロトコル**(prepared statement)を
使う。`aruaru-wire` の `describe_statement` / `describe_portal` は 2026-07-14
にクエリを実行せず構文解析 + スキーマ参照で列を解決する方式へ改修済みで、
`sqlx` / `Npgsql` / JDBC の `PreparedStatement` / psycopg の
server-side binding などが通る(`aruaru_commit` の二重実行も起きない)。
**2026-09-03 修正**: `SELECT col1, col2 ... AS OF COMMIT` の**列射影に対応**
(旧: 常にフル行を返していた)。`SELECT *` はフル行、列を並べれば
その順・その列だけ、不明な列名はエラー。通常の `SELECT` と同じ挙動。

### 5.1 結果列の型は現状すべて VARCHAR(text)

`aruaru-wire` は現在、関数以外の結果列を **`VARCHAR`(text フォーマット)**
で返す(内部ストレージが文字列のため)。したがって:

- **typed getter は使えない**: `row.get::<i32>(0)`(tokio-postgres)/
  `rs.getInt(1)`(JDBC)/ `cur.fetchone()[0]` を `int` として扱う 等は
  失敗しうる。**文字列で受けて `parse` / `Integer.parseInt` / `int()`**。
- ORM のマッピングは、列を `TEXT`/`String` として定義するか、読み出し後に
  アプリ側で変換する。
- `aruaru_commit()` の戻り値(commit_id)は元々 text なので影響なし。

型付き結果(INT4/TIMESTAMP 等の OID を返す)は今後の `aruaru-wire` の
課題(`CLAUDE.md` HANDOFF 参照)。

---

## 6. 検証状況(誇張しない)

- §3 のレシピは各エコシステムの**標準ドライバの標準的な使い方**であり、
  接続文字列とポート(5433 / 4001)以外に aruaru-db 固有の作法は無い。
- この開発環境で実際に往復検証できたもの:(このセッションでは
  `clients/*/README.md` に「未検証 / 検証済み」を明記する)。過去の
  セッションで **WSL2 実 PostgreSQL + `sqlx`(Rust)/ `psql`** に対する
  pgwire 往復は検証済み(`CLAUDE.md` 2026-07-13〜14 HANDOFF)。
- Java / Python / PHP / Go / Node / .NET の**ライブ往復**はこの環境に
  各ドライバが未導入のため本セッションでは未検証。`clients/` の例を
  実行して随時 README へ結果を追記する。

---
---

# English

## 0. Bottom line: no custom driver, no "device driver installer" needed

aruaru-db exposes only two **standard contracts**, both plain TCP:

| Contract | Default port | What it speaks |
|---|---|---|
| **PostgreSQL wire protocol (pgwire)** | `5433` | PostgreSQL-compatible: `SELECT` / `INSERT` / transactions / prepared statements / Git-on-SQL functions (`aruaru_commit` …) / `SELECT ... AS OF COMMIT '<id>'` |
| **GraphQL over HTTP** | `4001` (`/graphql`) | VersionlessAPI / Federation / observability / admin (`x-admin-token`) |

So **every** language (Java, Rust, Python, PHP, Go, Node, .NET, Ruby, …)
connects **with its existing PostgreSQL client** (JDBC / psycopg /
asyncpg / PDO_pgsql / pgx / node-postgres / Npgsql / ruby-pg). We ship
**no per-language driver** — you don't need one.

We deliberately do **not** build artifacts like
`<repo>-<lang>-<os>-device-driver-installer.exe`. They are not device
drivers (this is a network service), and re-implementing the mature,
security-audited PostgreSQL drivers for 5 languages × 8 OSes would be
exactly the "avoid blind replacements" anti-pattern forbidden by this
repo's `CLAUDE.md`. Instead, `clients/<stack>/` holds **ready-to-run
examples** (a connector kit). If an installer-style name is required, the
kit's distributable is named `aruaru-db-<lang>-connector` (e.g.
`aruaru-db-python-connector`); there is no `.exe` device-driver
installer. Any misspelling like
`aruaru-db-python-windows-device-driver-installer.exe` maps to
`aruaru-db-<lang>-connector`.

## 1. Quick matrix (language × framework × sync/async)

| Language | Sync driver | Async driver | Frameworks | Example |
|---|---|---|---|---|
| Java | JDBC (`org.postgresql:postgresql`) | R2DBC (`io.r2dbc:r2dbc-postgresql`) | Spring Boot / Quarkus / plain JDBC | `clients/java-jdbc/` |
| Rust | `postgres` | `tokio-postgres` / `sqlx` | Axum / Poem / RPoem | `clients/rust-axum/` |
| Python | `psycopg` v3 | `asyncpg` / `psycopg` async | FastAPI / Django / Flask+SQLAlchemy | `clients/python-fastapi/` |
| PHP | `PDO_pgsql` | (via Swoole/AMPHP) | Laravel (`pgsql`) / Symfony | `clients/php-laravel/` |
| Go | `database/sql` + `pgx` | `pgx` / `pgxpool` | net/http / Gin / Echo | `clients/go-pgx/` |
| Node/TS | — (JS is async) | `pg` / `postgres.js` | Express / Fastify / NestJS | `clients/node-pg/` |
| .NET | `Npgsql` (sync API) | `Npgsql` (async) / EF Core | ASP.NET Core | `clients/dotnet-npgsql/` |
| Ruby | `pg` gem | (via async gem) | Rails | recipe in §3.8 |

GraphQL from any language is just `POST /graphql` over HTTP — no special
client.

## 2. Connection strings

- libpq: `postgresql://<user>:<pass>@<host>:5433/<db>?sslmode=require`
- JDBC: `jdbc:postgresql://<host>:5433/<db>?ssl=true`
- R2DBC: `r2dbc:postgresql://<user>:<pass>@<host>:5433/<db>`
- GraphQL: `http(s)://<host>:4001/graphql`

## 3. Recipes

See the Japanese section §3 above — the code is language-only and
identical in both halves of this file. The Git-on-SQL bits
(`aruaru_commit`, `AS OF COMMIT '<id>'`) are **plain SQL**, so they are
the same string in every language.

## 4. Per-OS driver acquisition (no custom installer)

| OS / platform | Standard PostgreSQL client |
|---|---|
| Windows | JDBC jar / `pip install asyncpg` / `Install-Package Npgsql` / `npm i pg` / bundled `php_pdo_pgsql.dll` |
| macOS | same, plus `brew install libpq` |
| Linux | distro `libpq5` / `postgresql-client`; language packages as usual |
| UNIX (Solaris / AIX / *BSD) | JDBC (pure Java) / pkg `postgresql-client` / `libpq` |
| Mainframe (IBM z/OS) | **JDBC Type-4 (pure Java)** runs on the z/OS JVM as-is; native langs use `libpq` under USS |
| Android (phone / tablet) | JDBC dependency, or Ktor + `r2dbc-postgresql`, or OkHttp for GraphQL. Direct device→DB needs a network/auth design (usually go via an app server) |
| iOS / iPadOS | Swift: `PostgresClientKit` / `PostgresNIO` (SwiftNIO async); GraphQL via `URLSession` |

Anything that can open TCP (+ TLS) connects; the wire format is
PostgreSQL-compatible, so no aruaru-db-specific build is required.

## 5. Extended protocol (prepared statements)

Most ORMs/drivers default to the **extended protocol**. `aruaru-wire`'s
`describe_statement`/`describe_portal` were reworked on 2026-07-14 to
resolve columns from parse + schema lookup **without executing the
query** (so `sqlx` / `Npgsql` / JDBC `PreparedStatement` / psycopg
server-side binding all work, and `aruaru_commit` is not double-run).
**Fixed 2026-09-03**: column projection in `SELECT c1, c2 ... AS OF
COMMIT` **is now honored** (previously the full row was always returned).
`SELECT *` keeps the full row; listing columns returns those columns in
that order; an unknown column name errors. Same as a normal `SELECT`.

**5.1 — result columns currently all come back as `VARCHAR` (text).**
`aruaru-wire` currently returns non-function result columns as `VARCHAR`
(text format) because internal storage is string-based. So typed getters
(`row.get::<i32>(0)`, `rs.getInt(1)`, treating `cur.fetchone()[0]` as an
int, …) can fail — **read as string and `parse` / `Integer.parseInt` /
`int()`**. `aruaru_commit()`'s return (the commit id) is text anyway.
Typed results (real INT4/TIMESTAMP OIDs) are future `aruaru-wire` work.

## 6. Verification status (no exaggeration)

The §3 recipes are the standard use of each ecosystem's standard driver;
nothing aruaru-db-specific beyond ports 5433 / 4001. Previously verified
in this repo: pgwire round-trip against a real WSL2 PostgreSQL via
`sqlx` (Rust) and `psql` (`CLAUDE.md` HANDOFF 2026-07-13/14). Live
round-trips for Java / Python / PHP / Go / Node / .NET are not yet run in
this environment (drivers not installed here); `clients/*/README.md`
records "verified / not yet verified" per example.
