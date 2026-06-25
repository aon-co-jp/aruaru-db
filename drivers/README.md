# aruaru-DB 言語別ドライバー

## 基本原則

aruaru-DB は **PostgreSQL ワイヤプロトコル完全互換** です。
既存の PostgreSQL ドライバがそのまま接続できます。
各言語の専用ドライバー (`aruaru-db-{言語}`) は、
Git-on-SQL 機能を型付き API で使いやすくした**ラッパー層**です。

```
あなたのアプリ
    │
    ▼  aruaru-db-{言語} ドライバー (このディレクトリ)
    │  または既存の PostgreSQL ドライバ (psycopg3 / pgx / JDBC / Npgsql / ...)
    ▼
aruaru-server (pgwire: port 5432)
    │
    ▼  内部
aruaru-core (Git-on-SQL + HTAP Engine + fjall 永続化)
```

---

## 命名規則（各言語の慣例に従う）

ドライバーのパッケージ名・クラス名・モジュール名は
**各言語のエコシステム慣例**に従っています。

| 言語 | ディレクトリ | パッケージ名 | クラス / 型名 | 名前空間 / モジュール |
|---|---|---|---|---|
| Rust | `aruaru-db-rust/` | `aruaru-db-rust` (crates.io) | `AruaruDb` (struct) | crate root |
| Java | `aruaru-db-java/` | `dev.aruaru:aruaru-db-java` (Maven) | `AruaruDb` (class) | `dev.aruaru` |
| Python | `aruaru-db-python/` | `aruaru-db-python` (PyPI) | `AruaruDb` / `AruaruDbSync` (class) | `aruaru_db` (module) |
| Node.js/TS | `aruaru-db-nodejs/` | `@aruaru-db/nodejs` (npm) | `AruaruDb` (class) | named export |
| Go | `aruaru-db-go/` | `github.com/aruaru-db/aruaru-db-go` | `Client` (struct) | `package aruarudb` |
| PHP | `aruaru-db-php/` | `aruaru-db/aruaru-db-php` (Composer) | `AruaruDb` (class) | `namespace AruaruDB` |
| Ruby | `aruaru-db-ruby/` | `aruaru-db-ruby` (RubyGems) | `Client` (class) | `module AruaruDB` |
| .NET | `aruaru-db-dotnet/` | `AruaruDB.Dotnet` (NuGet) | `AruaruDb` (class) | `namespace AruaruDB` |
| C/C++ | `aruaru-db-cpp/` | `aruaru-db-cpp` (ヘッダオンリー) | `aruaru_db_*` (C 関数) / `AruaruDB::Client` (C++) | `namespace AruaruDB` |

### 命名規則の根拠

**クラス名を `AruaruDb` に統一した理由**
- `AruaruDB` ではなく `AruaruDb` を選択。各言語の略語慣例（Java: `HttpClient`、Rust: `TcpStream`）に合わせ、頭字語の全大文字を避けた。
- Go だけ例外として `Client` を採用。Go の慣例では「パッケージ名で文脈を示し、型名はシンプルに」するため（`aruarudb.Client` と書く）。
- Ruby も同様に `AruaruDB::Client` を採用。モジュールで名前空間を提供するのが Ruby 慣例。

**パッケージ名を `aruaru-db-{言語}` に統一した理由**
- どの言語のドライバーか一目でわかる。
- crates.io / PyPI / npm / Maven / RubyGems / NuGet / Composer で一貫した検索性。
- Go と C++ は言語エコシステムの慣例上、完全一致はしないが識別子に言語を含める。

---

## ディレクトリ構成

```
drivers/
├── README.md                  ← このファイル
├── aruaru-db-rust/            ← Rust ネイティブドライバー
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs             ← AruaruDb struct
│       ├── error.rs
│       └── types.rs
├── aruaru-db-java/            ← Java ドライバー
│   └── AruaruDriver.java      ← AruaruDb class
├── aruaru-db-python/          ← Python ドライバー
│   └── aruaru_db.py           ← AruaruDb / AruaruDbSync class
├── aruaru-db-nodejs/          ← Node.js / TypeScript ドライバー
│   └── index.ts               ← AruaruDb class
├── aruaru-db-go/              ← Go ドライバー
│   └── aruaru_db.go           ← package aruarudb / Client struct
├── aruaru-db-php/             ← PHP ドライバー
│   └── AruaruDriver.php       ← AruaruDB\AruaruDb class
├── aruaru-db-ruby/            ← Ruby ドライバー
│   └── aruaru_db.rb           ← AruaruDB::Client class
├── aruaru-db-dotnet/          ← .NET (C#) ドライバー
│   └── AruaruDriver.cs        ← AruaruDB.AruaruDb class
└── aruaru-db-cpp/             ← C / C++ ドライバー
    └── aruaru_db.h            ← C: aruaru_db_* 関数 / C++: AruaruDB::Client
```

---

## クイック接続テスト（どの言語でも）

aruaru-DB は PostgreSQL 互換なので、
psql でそのまま動作確認できます。

```bash
psql -h localhost -p 5432 -U root -d aruaru
```

```sql
-- 接続確認
SELECT aruaru_version();

-- Git-on-SQL 動作確認
SELECT aruaru_current_branch();
SELECT * FROM aruaru_log LIMIT 5;

-- ブランチ作成・コミット
SELECT aruaru_branch('feature/test');
CREATE TABLE hello (id INT, msg TEXT);
INSERT INTO hello VALUES (1, 'Hello aruaru-DB');
SELECT aruaru_commit('first commit');
```

---

## 言語別クイックスタート

### Rust (`aruaru-db-rust`)

```toml
# Cargo.toml
[dependencies]
aruaru-db-rust = "0.5"
tokio = { version = "1", features = ["full"] }
```

```rust
use aruaru_db_rust::AruaruDb;

#[tokio::main]
async fn main() -> aruaru_db_rust::Result<()> {
    let db = AruaruDb::connect("aruaru://root@localhost:5432/aruaru").await?;

    db.branch("feature/rust-test").await?;
    db.execute("CREATE TABLE tasks (id INT, title TEXT)", &[]).await?;
    db.execute("INSERT INTO tasks VALUES (1, 'Hello')", &[]).await?;

    let commit_id = db.commit("Add tasks via aruaru-db-rust").await?;
    println!("Committed: {commit_id}");

    for entry in db.log(5).await? {
        println!("{} {}: {}", entry.short_id, entry.author, entry.message);
    }
    Ok(())
}
```

#### Tauri v2 での利用（専用ドライバー不要）

**aruaru-DB に Tauri 専用ドライバーは存在しません。`aruaru-db-rust` がそのまま Tauri v2 で動作します。**

Tauri v2 は内部で Tokio を使っており、tokio-postgres ベースの `aruaru-db-rust` と完全に互換です。
以下のパターンで Tauri アプリに組み込めます。

```toml
# src-tauri/Cargo.toml
[dependencies]
aruaru-db-rust = "0.5"
tauri   = { version = "2", features = ["rustls-tls"] }
tokio   = { version = "1", features = ["full"] }
serde   = { version = "1", features = ["derive"] }
```

```rust
// src-tauri/src/main.rs
use std::sync::Arc;
use tokio::sync::Mutex;          // ← std::sync::Mutex ではなく tokio の Mutex を使う
use tauri::{command, State, Manager};
use aruaru_db_rust::AruaruDb;

// Tauri State として共有する型
struct AruaruState(Arc<Mutex<AruaruDb>>);

// ── Tauri コマンド ──────────────────────────────────────────

#[command]
async fn db_branch(name: String, state: State<'_, AruaruState>) -> Result<(), String> {
    let db = state.0.lock().await;
    db.branch(&name).await.map_err(|e| e.to_string())
}

#[command]
async fn db_commit(message: String, state: State<'_, AruaruState>) -> Result<String, String> {
    let db = state.0.lock().await;
    db.commit(&message).await.map_err(|e| e.to_string())
}

#[command]
async fn db_execute(sql: String, state: State<'_, AruaruState>) -> Result<u64, String> {
    let db = state.0.lock().await;
    db.execute(&sql, &[]).await.map_err(|e| e.to_string())
}

#[command]
async fn db_log(limit: i32, state: State<'_, AruaruState>) -> Result<Vec<serde_json::Value>, String> {
    let db = state.0.lock().await;
    let entries = db.log(limit as usize).await.map_err(|e| e.to_string())?;
    Ok(entries.iter().map(|e| serde_json::json!({
        "id": e.id, "short_id": e.short_id,
        "author": e.author, "message": e.message,
        "timestamp": e.timestamp,
    })).collect())
}

// ── エントリポイント ────────────────────────────────────────

#[tokio::main]
async fn main() {
    let db = AruaruDb::connect("aruaru://root@localhost:5432/aruaru")
        .await
        .expect("aruaru-DB 接続失敗");
    let state = AruaruState(Arc::new(Mutex::new(db)));

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            db_branch,
            db_commit,
            db_execute,
            db_log,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 起動失敗");
}
```

```typescript
// フロントエンド (TypeScript)
import { invoke } from "@tauri-apps/api/core";

await invoke("db_branch",  { name: "feature/tauri-test" });
await invoke("db_execute", { sql: "INSERT INTO tasks VALUES (1, 'Hello')" });
const commitId = await invoke<string>("db_commit", { message: "Add task" });
const log      = await invoke<object[]>("db_log",   { limit: 10 });
```

> **重要な注意点**
>
> | 項目 | 正しい使い方 | 誤った使い方 |
> |---|---|---|
> | Mutex | `tokio::sync::Mutex` | `std::sync::Mutex`（async コマンドでエラー）|
> | 接続管理 | `Arc<Mutex<AruaruDb>>` で共有 | コマンドごとに毎回接続（非効率）|
> | 大量行の取得 | `spawn_blocking` か limit を使う | 長時間ブロック（UI フリーズ）|
>
> Tauri 公式の `tauri-plugin-sql` は汎用 PostgreSQL クライアントで、
> aruaru-DB 固有の `aruaru_branch()` / `aruaru_commit()` などの
> Git-on-SQL 関数は呼び出せません。
> **`aruaru-db-rust` を使うことで Git-on-SQL の全機能が利用できます。**

---

### Java (`dev.aruaru:aruaru-db-java`)

```xml
<!-- pom.xml -->
<dependency>
    <groupId>dev.aruaru</groupId>
    <artifactId>aruaru-db-java</artifactId>
    <version>0.5.0</version>
</dependency>
```

```java
import dev.aruaru.AruaruDb;

AruaruDb db = new AruaruDb("localhost", 5432, "aruaru", "root");

db.branch("feature/java-test");
db.execute("CREATE TABLE tasks (id INT, title TEXT)");
db.execute("INSERT INTO tasks VALUES (?, ?)", 1, "Hello");

String commitId = db.commit("Add tasks via aruaru-db-java");
System.out.println("Committed: " + commitId);

db.log(5).forEach(System.out::println);
db.close();
```

---

### Python (`aruaru-db-python`)

```bash
pip install aruaru-db-python
```

```python
# 非同期 (推奨)
import asyncio
from aruaru_db import AruaruDb

async def main():
    db = await AruaruDb.connect("postgres://root@localhost:5432/aruaru")
    await db.branch("feature/python-test")
    await db.execute("CREATE TABLE tasks (id INT, title TEXT)")
    await db.execute("INSERT INTO tasks VALUES ($1, $2)", 1, "Hello")
    commit_id = await db.commit("Add tasks via aruaru-db-python")
    print("Committed:", commit_id)
    for e in await db.log(5):
        print(f"{e.short_id}  {e.author}: {e.message}")
    await db.close()

asyncio.run(main())
```

```python
# 同期版
from aruaru_db import AruaruDbSync

with AruaruDbSync.connect() as db:
    db.branch("feature/sync-test")
    db.execute("INSERT INTO tasks VALUES (%s, %s)", (2, "World"))
    print(db.commit("Sync commit"))
```

---

### Node.js / TypeScript (`@aruaru-db/nodejs`)

```bash
npm install @aruaru-db/nodejs
```

```typescript
import { AruaruDb } from "@aruaru-db/nodejs";

const db = new AruaruDb({ host: "localhost" });

await db.branch("feature/nodejs-test");
await db.execute`CREATE TABLE tasks (id INT, title TEXT)`;
await db.execute`INSERT INTO tasks VALUES (${1}, ${"Hello"})`;

const commitId = await db.commit("Add tasks via aruaru-db-nodejs");
console.log("Committed:", commitId);

const log = await db.log(5);
log.forEach(e => console.log(`${e.short_id}  ${e.author}: ${e.message}`));

await db.end();
```

---

### Go (`github.com/aruaru-db/aruaru-db-go`)

```bash
go get github.com/aruaru-db/aruaru-db-go
```

```go
import (
    "context"
    aruarudb "github.com/aruaru-db/aruaru-db-go"
)

ctx := context.Background()
db, err := aruarudb.Connect(ctx, "postgres://root@localhost:5432/aruaru")
if err != nil { log.Fatal(err) }
defer db.Close()

_ = db.Branch(ctx, "feature/go-test")
_, _ = db.Execute(ctx, "CREATE TABLE tasks (id INT, title TEXT)")
_, _ = db.Execute(ctx, "INSERT INTO tasks VALUES ($1, $2)", 1, "Hello")

commitID, _ := db.Commit(ctx, "Add tasks via aruaru-db-go")
fmt.Println("Committed:", commitID)

entries, _ := db.Log(ctx, 5)
for _, e := range entries {
    fmt.Printf("%s  %s: %s\n", e.ShortID, e.Author, e.Message)
}
```

---

### PHP (`aruaru-db/aruaru-db-php`)

```bash
composer require aruaru-db/aruaru-db-php
```

```php
use AruaruDB\AruaruDb;

$db = new AruaruDb(host: 'localhost', db: 'aruaru', user: 'root');

$db->branch('feature/php-test');
$db->execute("CREATE TABLE tasks (id INT, title TEXT)");
$db->execute("INSERT INTO tasks VALUES (?, ?)", [1, 'Hello']);

$commitId = $db->commit('Add tasks via aruaru-db-php');
echo "Committed: $commitId\n";

foreach ($db->log(5) as $row) {
    echo "{$row['short_id']}  {$row['author']}: {$row['message']}\n";
}
```

---

### Ruby (`aruaru-db-ruby`)

```bash
gem install aruaru-db-ruby
```

```ruby
require 'aruaru_db'

db = AruaruDB::Client.connect(url: 'postgres://root@localhost:5432/aruaru')

db.branch('feature/ruby-test')
db.execute("CREATE TABLE tasks (id INT, title TEXT)")
db.execute("INSERT INTO tasks VALUES ($1, $2)", 1, 'Hello')

commit_id = db.commit('Add tasks via aruaru-db-ruby')
puts "Committed: #{commit_id}"

db.log(limit: 5).each do |e|
  puts "#{e.short_id}  #{e.author}: #{e.message}"
end

db.close
```

---

### .NET / C# (`AruaruDB.Dotnet`)

```bash
dotnet add package AruaruDB.Dotnet
```

```csharp
using AruaruDB;

await using var db = new AruaruDb("localhost", 5432, "aruaru", "root");

await db.BranchAsync("feature/dotnet-test");
await db.ExecuteAsync("CREATE TABLE tasks (id INT, title TEXT)");
await db.ExecuteAsync("INSERT INTO tasks VALUES ($1, $2)", 1, "Hello");

var commitId = await db.CommitAsync("Add tasks via aruaru-db-dotnet");
Console.WriteLine($"Committed: {commitId}");

foreach (var entry in await db.LogAsync(5))
    Console.WriteLine($"{entry["short_id"]}  {entry["author"]}: {entry["message"]}");
```

---

### C / C++ (`aruaru-db-cpp`)

```cpp
// C++17
#include "aruaru_db.h"

// C++ RAII スタイル
AruaruDB::Client db("localhost", 5432, "aruaru", "root");
db.branch("feature/cpp-test");
auto commitId = db.commit("Add tasks via aruaru-db-cpp");
```

```c
/* C スタイル */
#include "aruaru_db.h"

AruaruDBConn* conn = aruaru_db_connect("localhost", 5432, "aruaru", "root");
aruaru_db_branch(conn, "feature/c-test");
char* commit_id = aruaru_db_commit(conn, "Add tasks via aruaru-db-cpp");
free(commit_id);
aruaru_db_close(conn);
```

---

## v0.6 予定: フレームワーク統合骨格

v0.6 では各言語・フレームワークへの統合骨格を追加します。
言語ドライバー単体（現在の `aruaru-db-{言語}`）で動く設計は変わりません。

| 言語 | フレームワーク | 統合パッケージ (予定) |
|---|---|---|
| Rust | Poem, Axum, Actix-web | `aruaru-db-rust` の feature flag で対応 |
| Java | Spring Boot | `aruaru-db-spring` (Spring Data 互換) |
| Python | FastAPI, Django | `aruaru-db-python` の async 対応で対応 |
| Node.js | Express, NestJS | `@aruaru-db/nodejs` の middleware で対応 |
| Go | gin, Echo, chi | `aruaru-db-go` の middleware パッケージで対応 |
| PHP | Laravel | `aruaru-db-laravel` (Eloquent 互換) |
| Ruby | Rails | `aruaru-db-rails` (ActiveRecord 互換) |
| .NET | ASP.NET Core | `AruaruDB.AspNetCore` (DI 拡張) |

> **注意**: v0.5 現在、フレームワーク統合は未実装です。
> 言語ドライバーを直接 `new AruaruDb(...)` / `AruaruDb::connect(...)` で
> 使うことで、どのフレームワーク上でも動作します。

---

## PostgreSQL 互換ドライバーで直接使う場合

aruaru-DB は PostgreSQL ワイヤ互換なので、
専用ドライバーを使わず既存の PostgreSQL ドライバーで直接接続できます。

| 言語 | 推奨ドライバー | 接続文字列 |
|---|---|---|
| Rust | `tokio-postgres` / `sqlx` | `postgres://root@localhost:5432/aruaru` |
| Java | PostgreSQL JDBC 42+ | `jdbc:postgresql://localhost:5432/aruaru` |
| Python | `asyncpg` / `psycopg3` | `postgres://root@localhost:5432/aruaru` |
| Node.js | `postgres.js` / `pg` | `postgres://root@localhost:5432/aruaru` |
| Go | `pgx/v5` | `postgres://root@localhost:5432/aruaru` |
| PHP | `pdo_pgsql` | `pgsql:host=localhost;port=5432;dbname=aruaru` |
| Ruby | `pg` gem | `postgres://root@localhost:5432/aruaru` |
| .NET | `Npgsql` | `Host=localhost;Port=5432;Database=aruaru;Username=root` |
| C/C++ | `libpq` | `host=localhost port=5432 dbname=aruaru user=root` |

Git-on-SQL の関数は SQL で直接呼び出せます:

```sql
SELECT aruaru_branch('feature/my-branch');
SELECT aruaru_commit('my commit message');
SELECT * FROM aruaru_log LIMIT 10;
SELECT aruaru_merge('feature/my-branch');
```

---

*Apache-2.0 License — aruaru-DB Project*
