# aruaru-DB Language Drivers

## 基本原則

aruaru-DB は **PostgreSQL ワイヤプロトコル完全互換** のため、  
既存の PostgreSQL ドライバがそのまま動作します。  
各言語の専用ラッパー (`aruaru-*`) は、Git-on-SQL 機能を使いやすくする追加層です。

```
あなたのアプリ
    ↓ 既存の PostgreSQL ドライバ (psycopg3 / pg / JDBC / Npgsql / ...)
aruaru-server (pgwire: port 5432)
    ↓ 内部
aruaru-core (Git-on-SQL + HTAP Engine)
```

## クイック接続テスト (どの言語でも)

```sql
-- 接続確認
SELECT version();
SELECT aruaru_version();

-- ブランチ確認
SELECT * FROM aruaru_log LIMIT 5;

-- コミット
SELECT aruaru_commit('my first commit');
```

## 各言語のディレクトリ

| ディレクトリ | 言語 | パッケージ名 |
|---|---|---|
| `python/` | Python 3.10+ | `aruaru-py` (PyPI) |
| `nodejs/` | Node.js 20+ / TypeScript | `@aruaru-db/client` (npm) |
| `java/` | Java 17+ | `dev.aruaru:client` (Maven) |
| `dotnet/` | .NET 8+ | `Aruaru.Client` (NuGet) |
| `go/` | Go 1.22+ | `github.com/aruaru-db/aruaru-go` |
| `php/` | PHP 8.2+ | `aruaru-db/client` (Composer) |
| `ruby/` | Ruby 3.2+ | `aruaru-db` (RubyGems) |
| `cpp/` | C / C++17 | `libaruaru` |
| `kotlin/` | Kotlin 2.0+ | `dev.aruaru:kotlin-client` |
| `swift/` | Swift 5.9+ | `AruaruDB` (SPM) |
| `native/` | Rust | `aruaru-client` (crates.io) |
