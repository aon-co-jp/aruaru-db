# aruaru-DB 🦀

> **The hybrid distributed database that speaks Git.**  
> CockroachDB's distributed strong consistency × Snowflake's storage/compute separation × Git-on-SQL versioning — all in Pure Rust.

[![Version](https://img.shields.io/badge/version-0.5.1-orange.svg)](https://github.com/aruaru-db/aruaru-db/releases)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/aruaru-db/aruaru-db/actions/workflows/ci.yml/badge.svg)](https://github.com/aruaru-db/aruaru-db/actions)
[![Discord](https://img.shields.io/badge/Discord-community-5865F2.svg)](https://discord.gg/aruaru-db)

---

## ✨ Why aruaru-DB

| Feature | CockroachDB | Snowflake | **aruaru-DB** |
|------|------------|-----------|---------------|
| Distributed strong consistency (Raft) | ✅ | ❌ | ✅ |
| Storage/compute separation | ❌ | ✅ | ✅ |
| Columnar OLAP (Arrow/DataFusion) | ❌ | ✅ | ✅ |
| Git-on-SQL (branch / merge / diff) | ❌ | ❌ | ✅ |
| Versionless GraphQL API | ❌ | ❌ | ✅ |
| Tauri admin GUI | ❌ | ❌ | ✅ |
| Migration tooling (Postgres / MySQL / CSV) | △ | △ | ✅ |
| **Fully open source (Apache-2.0)** | ❌ (since 2024) | ❌ | ✅ |
| Pure Rust | ❌ (Go) | ❌ | ✅ |

---

## 🏗️ Architecture Overview

```
┌──────────────────────────────────────────────────────────┐
│  Layer 3 : Access                                        │
│  pgwire (PostgreSQL wire compatible) │ GraphQL (Poem/async-graphql) │
│  REST API                            │ Tauri Admin GUI             │
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

### Three-layer defense transport (`aruaru-wire`)

`aruaru-wire` is both the pgwire implementation and the carrier of the three-layer
defense transport: TLS (layer 1) + mutual authentication (layer 2) + payload
encryption (layer 3). **open-web-server** and **open-runo** implement the same
three layers in their own `open-web-server-wire` crate, so communication across all
three projects is held to a single, consistent security bar.

---

## 🔗 Integration with open-web-server and open-runo

aruaru-DB works standalone, but for workloads like 3D online game item billing or
financial data, it's typically deployed as the third hop in a three-tier chain:

```text
Client → open-web-server (entry point, WAL pre-write)
       → open-runo (Federation Gateway — centralizes auth/audit)
       → aruaru-DB (Raft consensus commit, issues commit_id)
```

A write is not considered final until a `commit_id` is issued, so mid-flight
network hiccups or retries never result in a duplicate write. See
`docs/integration.md` in the open-web-server repository for the full protocol.

---

## 🧬 ARU3 Triple-Write Storage (Layer 3 Detailed Design)

"Paid items and balances stay written the moment they're written" — that promise is
implemented as an append-only engine in a custom binary format called **ARU3**. Full
spec: [`docs/FORMAT-ARU3-Japan.md`](docs/FORMAT-ARU3-Japan.md).

```
open-web-server (Layer 1) → open-runo (Layer 2) → aruaru-db (Layer 3) × 3 replicas
```

aruaru-db is intentionally simple: it's written to only by open-runo, and it just
reads and writes. Congestion control, retries, and priority-lane decisions all live
in Layer 2 — aruaru-db focuses purely on durability and integrity.

### Core Technique: Dual Checksums

Every record carries two BLAKE3 checksums:

- `payload_checksum` — computed end-to-end by open-web-server and stored as-is.
- `record_checksum` — computed locally by aruaru-db immediately before the write.

Verifying both on read lets the system **distinguish network-path corruption from
on-disk corruption** — that distinction is what triggers the right repair action.

### Key Features

- **Append-only** — never overwritten, so failure can only ever look like a "torn write," which is simple to detect and recover from.
- **Self-verifying records** — recoverable by scanning for the magic number from any offset, not just the start of the file.
- **WAL + fsync boundaries** — `FINANCE`-flagged records block on WAL fsync then segment fsync before acknowledging. `GAME` writes use group commit for throughput.
- **Quorum reads** — reads consult all three replicas and return the checksum-majority value. Financial balance queries can force `require_full_quorum` (3/3 agreement).
- **Crash recovery** — on startup, any torn record at a segment's tail is truncated automatically and replayed from the WAL.

### Boundary Design: Separation of Concerns with open-runo

aruaru-db keeps its own internal representation, `Record` (the ARU3 format) —
deliberately. Converting from `Commit` (the shared network-facing type) to `Record`
(the on-disk persistent format) is explicitly open-runo's responsibility. That means
the storage layer's internal format can evolve without rippling up into Layer 1 or
the client. What should be shared (`aruaru-common`) and what should stay private to a
layer (`Record`) are kept intentionally separate.

```rust
use aruaru_db::TripleStore;
use std::path::Path;

let mut store = TripleStore::open([
    Path::new("f:/open-aruaru/aruaru-db/data/r0"),
    Path::new("e:/open-aruaru/aruaru-db/data/r1"),
    Path::new("d:/aruaru-db/data/r2"),
])?;

// Normally called from open-runo, but usable directly for unit tests
// or embedded deployments.
```

- aruaru-db never references the open-runo/open-web-server proto — the coupling is
  intentionally minimal. The only contract point is the `Record` struct's
  `payload_checksum` field, which carries the BLAKE3 value computed back at Layer 1
  unchanged.
- The three directories passed to `TripleStore::open` should map 1:1 to open-runo's
  `ARU3_R0/R1/R2` environment variables.

---

## 🚀 Quick Start

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

## 📦 Crate Layout

| Crate | Role |
|---------|------|
| `aruaru-core` | Storage engine, MVCC, Git-on-SQL versioning |
| `aruaru-dist` | openraft integration, range sharding, node management |
| `aruaru-query` | SQL parser, HTAP router, DataFusion integration |
| `aruaru-wire` | PostgreSQL wire protocol (pgwire) + 3-layer defense transport |
| `aruaru-graphql` | Versionless GraphQL + Poem HTTP server |
| `aruaru-migrate` | Postgres / CockroachDB / Snowflake / CSV migration tooling |
| `aruaru-server` | Main binary — integrated entry point for all crates |

---

## 🌿 Using Git-on-SQL

```sql
-- create a branch
SELECT aruaru_branch('feature/new-schema');

-- alter a table on the current branch
ALTER TABLE users ADD COLUMN score INT DEFAULT 0;

-- commit
SELECT aruaru_commit('Add score column to users');

-- view the log
SELECT * FROM aruaru_log LIMIT 10;

-- diff
SELECT * FROM aruaru_diff('main', 'feature/new-schema');

-- merge
SELECT aruaru_merge('feature/new-schema', 'main');
```

---

## 🤝 Contributing

Maintained by volunteers around the world.

- **Issues**: file bug reports and feature requests on GitHub Issues
- Start with anything labeled **good-first-issue**
- Please read `CONTRIBUTING.md` before opening a PR
- Discord: join the community channel for discussion

---

## 📄 License

Apache License 2.0 — free to use, modify, and redistribute commercially.  
© 2026 aruaru-DB Contributors
