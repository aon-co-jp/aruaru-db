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

```sql
-- Create a branch
SELECT aruaru_branch('feature/new-schema');

-- Modify a table on the current branch
ALTER TABLE users ADD COLUMN score INT DEFAULT 0;

-- Commit
SELECT aruaru_commit('Add score column to users');

-- Check the log
SELECT * FROM aruaru_log LIMIT 10;

-- diff
SELECT * FROM aruaru_diff('main', 'feature/new-schema');

-- Merge
SELECT aruaru_merge('feature/new-schema', 'main');
```

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
