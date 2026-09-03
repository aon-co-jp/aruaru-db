# aruaru-db コネクタキット / Connector Kit

**正本は [`../docs/CLIENTS.md`](../docs/CLIENTS.md)**。ここには「そのまま
動く最小サンプル」を言語 × フレームワーク別に置く。

## 方針(なぜ「言語別ドライバ」を作らないか)

aruaru-db は **PostgreSQL ワイヤプロトコル(:5433)** と
**GraphQL/HTTP(:4001)** の 2 契約だけを公開する。どちらも普通の TCP で、
各言語の**標準 PostgreSQL ドライバ**がそのまま繋がる。したがって:

- `aruaru-db-<言語>-<OS>-device-driver-installer.exe` のような成果物は
  **作らない**(デバイスドライバではないし、成熟した標準ドライバの
  再実装は `CLAUDE.md` の「闇雲な代替を避ける」原則に反する)。
- 同期でも非同期でもワイヤ上は同一。速度を犠牲にしないため、独自の
  変換層は挟まない。
- 配布物名が要る場合は `aruaru-db-<言語>-connector`(例
  `aruaru-db-python-connector`)= このディレクトリのサンプル一式。

## ディレクトリ

| ディレクトリ | スタック | 同期/非同期 | 検証状況 |
|---|---|---|---|
| `python-fastapi/` | Python + FastAPI + asyncpg | 非同期 | 未検証(この環境に asyncpg 未導入) |
| `rust-axum/` | Rust + Axum + sqlx(PgPool) | 非同期 | 未検証(過去に sqlx pgwire 往復は検証済み、CLAUDE.md 2026-07-13/14) |
| `php-laravel/` | PHP + Laravel + PDO_pgsql | 同期 | 未検証(この環境に PHP 未導入) |
| `java-jdbc/` | Java + 素の JDBC | 同期(R2DBC 例も併記) | 未検証(この環境に JDBC jar 未導入) |
| `rust-aruaru-db/` | **公式 Rust コネクタ** `aruaru-db-connector`(`tokio-postgres` + `commit()`/`query_as_of()`、RPoem/Axum/Poem 向け) | 非同期 | ネットワーク不要 2 + doctest = **green** |
| `python-aruaru-db/` | **公式 Python コネクタ** `aruaru-db`(`AruaruDb`=asyncpg / `AruaruDbSync`=psycopg。FastAPI/Django/Flask) | 両方 | ネットワーク不要 4 = **green** |
| `node-aruaru-db/` | **公式 Node コネクタ** `@aruaru/db`(`pg` の薄いラッパー。Express/Fastify/NestJS) | 非同期 | ネットワーク不要 4 = **green** |
| `php-aruaru-db/` | **公式 PHP コネクタ** `aruaru/db`(`PDO` の薄いラッパー。Laravel/Symfony) | 同期 | 未検証(この環境に PHP なし) |
| `cobol/` | COBOL 埋め込み SQL(`EXEC SQL`)参照実装。ODBC(psqlODBC)/ libpq / OCESQL、z/OS 含む | 同期 | 未検証(COBOL 環境なし) |

各サンプルの `README.md` に実行手順と、実際に往復検証したら結果を追記する。

## 共通の前提

```
# aruaru-server 起動例(TLS 無し・ローカル検証用)
aruaru-server --pg-port 5433 --gql-port 4001 --data ./data
# ARUARU_USERS で SCRAM 認証情報を設定
export ARUARU_USERS='app:secret'
```

Git-on-SQL の 2 機能(`SELECT aruaru_commit('msg')` と
`SELECT ... AS OF COMMIT '<id>'`)は**ただの SQL** なので、どのサンプルも
同じ文字列を投げるだけ。

---

# English

Source of truth: [`../docs/CLIENTS.md`](../docs/CLIENTS.md). This folder
holds minimal ready-to-run examples per language × framework.

aruaru-db exposes only the **PostgreSQL wire protocol (:5433)** and
**GraphQL/HTTP (:4001)**. Every language's standard PostgreSQL driver
connects as-is, sync or async (same bytes on the wire). We therefore do
**not** ship `aruaru-db-<lang>-<os>-device-driver-installer.exe`
artifacts — they are not device drivers, and re-implementing mature
audited drivers violates this repo's "avoid blind replacements"
principle. If a distributable name is needed it is
`aruaru-db-<lang>-connector` = the samples in this directory.
