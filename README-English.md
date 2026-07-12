# aruaru-DB 🦀

> **The hybrid distributed database that speaks Git.**  
> CockroachDB's distributed strong consistency × Snowflake's storage/compute separation × Git-on-SQL version control — all in Pure Rust.

[![Version](https://img.shields.io/badge/version-0.5.0-orange.svg)](https://github.com/aruaru-db/aruaru-db/releases)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/aruaru-db/aruaru-db/actions/workflows/ci.yml/badge.svg)](https://github.com/aruaru-db/aruaru-db/actions)
[![Discord](https://img.shields.io/badge/Discord-community-5865F2.svg)](https://discord.gg/aruaru-db)

📖 Other languages: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

---

## ✨ Why aruaru-DB

| Feature | CockroachDB | Snowflake | **aruaru-DB** |
|---|:---:|:---:|:---:|
| Distributed strong consistency (Raft) | ✅ | ❌ | ✅ |
| Storage/compute separation | ❌ | ✅ | ✅ |
| Columnar OLAP (Arrow/DataFusion) | ❌ | ✅ | ✅ |
| Git-on-SQL (branch / merge / diff) | ❌ | ❌ | ✅ |
| Versionless GraphQL API | ❌ | ❌ | ✅ |
| Tauri admin GUI | ❌ | ❌ | ✅ |
| Migration tools (Postgres / MySQL / CSV) | △ | △ | ✅ |
| **Fully OSS (Apache-2.0)** | ❌ (2024~) | ❌ | ✅ |
| Pure Rust | ❌ (Go) | ❌ | ✅ |

---

## 🏗️ Architecture overview

```
┌──────────────────────────────────────────────────────────┐
│  Layer 3 : Access                                        │
│  pgwire (PostgreSQL wire compat) │ GraphQL (Poem/async-graphql)│
│  REST API                 │  Tauri Admin GUI             │
├──────────────────────────────────────────────────────────┤
│  Layer 2 : Query & Distribution                          │
│  HTAP Router  │  DataFusion (OLAP)  │  openraft (Raft)  │
│  MVCC         │  Range Sharding     │  SQL Planner       │
├──────────────────────────────────────────────────────────┤
│  Layer 1 : Storage                                       │
│  Row Store (fjall LSM)  │  Columnar (Arrow / Parquet)   │
│  Version Tree (Prolly)  │  WAL (Write-Ahead Log)        │
└──────────────────────────────────────────────────────────┘
```

See [ARCHITECTURE.md](ARCHITECTURE.md) and [docs/DATABASE.md](docs/DATABASE.md) for details.

---

## 🚀 Quick start

```bash
# Start the server (PostgreSQL port 5432 + GraphQL :4000)
cargo run -p aruaru-server -- --data ./data --raft-id 1

# Connect with psql
psql -h localhost -U root -d aruaru

# GraphQL endpoint
open http://localhost:4000/graphql
```

### Tauri Admin GUI

```bash
cd admin
npm install
npm run tauri dev
```

---

## 📦 Crate layout

| Crate | Role |
|---|---|
| `aruaru-core` | Storage engine, MVCC, Git-on-SQL version control |
| `aruaru-dist` | openraft integration, range sharding, node management, Raft-commit x open-raid-z snapshot pairing (`snapshot_pairing`, added 2026-07-13) |
| `aruaru-query` | SQL parser, HTAP router, DataFusion integration |
| `aruaru-wire` | PostgreSQL wire protocol (pgwire) |
| `aruaru-graphql` | Versionless GraphQL + Poem HTTP server |
| `aruaru-registry` | Supported-DB registry (150+ entries), daily crawl, ingest adapters |
| `aruaru-migrate` | Postgres / CockroachDB / Snowflake / MySQL / CSV migration tool |
| `aruaru-backup` | Backup, restore, point-in-time recovery (Parquet) |
| `aruaru-server` | Main binary (integrated entry point for all crates) |

---

## 🌿 Using Git-on-SQL

> ⚠️ The previous version of this example used `ALTER TABLE` and
> `SELECT aruaru_diff(...)`, **neither of which the current SQL parser
> supports** (verified against source, 2026-07-12). Replaced below with
> syntax that actually works.

```sql
-- Create a branch, then switch to it
SELECT aruaru_branch('feature/new-schema');
SELECT aruaru_checkout('feature/new-schema');

-- Change data on this branch (assumes the table was already CREATE TABLE'd)
INSERT INTO users (id, name, score) VALUES (1, 'Alice', 100);

-- Commit
SELECT aruaru_commit('Add score for Alice');

-- Check the log
SELECT * FROM aruaru_log LIMIT 10;

-- Switch back to main, then fast-forward merge feature into it.
-- Note: aruaru_merge takes exactly ONE argument (the source branch) and
-- merges it into whatever the CURRENT branch is. The old two-argument
-- form aruaru_merge('feature/new-schema', 'main') shown in a previous
-- version of this README does not match the implementation and will not work.
SELECT aruaru_checkout('main');
SELECT aruaru_merge('feature/new-schema');
```

Branch diffs aren't exposed as a SQL function — use the `aruaru-graphql` API instead:

```graphql
query {
  diff(from: "main", to: "feature/new-schema") {
    added
    removed
    modified
  }
}
```

### UPSERT (added 2026-07-12)

`ON CONFLICT ... DO UPDATE` / `DO NOTHING` is now supported (added for
compatibility with the UPSERT SQL that `open-runo` generates):

```sql
-- First call inserts a new row; on a later call with the same id, only
-- the balance column is overwritten with EXCLUDED (the new value passed in).
INSERT INTO wallets (id, balance) VALUES (1, '500')
  ON CONFLICT (id) DO UPDATE SET balance = EXCLUDED.balance;

-- Idempotent "create if missing" pattern: do nothing if it already exists
INSERT INTO wallets (id, balance) VALUES (1, '500')
  ON CONFLICT (id) DO NOTHING;
```

> Conflict detection currently only considers the table's **first column**
> (which this engine always treats as the primary key). The `col` in
> `ON CONFLICT (col)` must match that first column, or the statement returns
> an error.

---

## 🔗 Related projects

There is a target architecture combining `open-web-server` with
`poem-cosmo-tauri`/`open-runo`, PostgreSQL, and `open-raid-z` (revised
2026-07-11): quadruple-redundant transport (TCP-IP/UDP-IP/QUIC
(MPQUIC)/MPTCP or SCTP) and quadruple-redundant DB writes
(PostgreSQL/aruaru-db/multi-region synchronous replication/an independent
audit log), designed to prevent loss of paid-item and financial/securities
data in 3D online games. aruaru-db participates as the distributed
Git-on-SQL data layer, and in the hybrid of VersionLess API and
Git-managed versioning. Currently only TCP-IP/UDP-IP are implemented; the
rest has not been started yet (see `open-web-server`'s
`README.md`/`CLAUDE.md` for details).

---

## 🤝 Contributing

Maintained by volunteers around the world.

- **Issues**: report bugs and propose features via GitHub Issues
- Start with a **good-first-issue** label
- Please read `CONTRIBUTING.md` first
- Discord: discuss in the community channel

---

## 📄 License

Apache License 2.0 — free to use commercially, modify, and redistribute.  
© 2026 aruaru-DB Contributors
