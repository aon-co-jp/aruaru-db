# aruaru-DB 🦀

> **Updated 2026-08-29 (pivoted to a ground-up redesign of the admin
> plane)**: aruaru-db is designed to be used **as a SET (paired) with
> RPoem** — only together do they deliver "no REST API needed,
> compatible with WunderGraph Cosmo's paid Enterprise tier". Moving
> `/admin/*` REST endpoints to GraphQL mutations one by one turned out
> to be *just relocating an anti-pattern* (live per-field mutation of a
> running process's internal state). In response, a canonical design
> document — **[`docs/CONTROL_PLANE_REDESIGN.md`](docs/CONTROL_PLANE_REDESIGN.md)** —
> was created and the work pivoted to a ground-up redesign.
> New design philosophy (§2, 12 principles): **express everything as a
> declaration of desired state plus reconciliation; never place
> imperative RPC on the data plane** — the common solution shared by
> Kubernetes/GitOps, WunderGraph Cosmo, TiDB/TiFlash and SPIFFE. The
> data plane (`aruaru-server`) will ultimately expose only `/graphql`,
> `/graphql/sdl`, `/health*` and `/metrics`; **all REST APIs, `/admin/*`
> included, are being removed completely**. Operational config becomes a
> declarative `aruaru.yaml` with hot reload (no runtime mutations). API
> keys have a fully automatic lifecycle (self-issue / self-approve /
> self-revoke / self-expire).
>
> Progress (of phases P0–P6, as of 2026-08-31): P0 design frozen /
> P1 declarative-config foundation (`aruaru-server::config`, `--config`,
> hot reload) done / P2 `query.parallel` (reduced to 4 fields) and
> `follower_read.target_lag_ms` (fully hot-reloadable) done / **P3 main
> body**: on top of `/admin/parallel*` and `/v1/keys/self-issue` (cont. 8),
> the **`closed-timestamp`, `wal-service` and `sharded-store` REST
> endpoints are now fully removed and moved to GraphQL query/mutation**
> (cont. 10): `closedTimestamp`/`planFollowerRead`/`closedTsRegisterRange`/
> `closedTsAdvance`, `walService`/`walPage`/`walAppend`/
> `walCreateImageLayer`, `shardedStoreGet`/`shardedStoreStats`/
> `shardedStorePut`. `ephemeral-query` and `multi-raft` need a trait-
> injection refactor and are deferred to the next slice.
> **Appendix A has been substantially expanded as a "2026 state-of-the-art
> design"** (re-researched via primary papers / official docs / GitHub in
> English, Japanese and German): TiDB/TiFlash's DeltaTree (a B+tree × LSM
> hybrid), CockroachDB's Range/HLC/Pebble/closed timestamp, YugabyteDB's
> DocDB, Snowflake's immutable micro-partitions + pruning + time travel,
> Neon vs Aurora WAL disaggregation, SingleStore Universal Storage,
> ClickHouse MergeTree/SharedMergeTree, an Iceberg/Delta/Hudi comparison,
> and Photon/DuckDB type-aware lightweight compression (FSST/ALP) — all
> down to **implementation methods (architecture, data structures,
> algorithms)** — leading to decisions to adopt Raft-Learner row→column
> replicas, HLC and deletion vectors (with reasons stated for what is
> *not* adopted). Appendix B documents the Cosmo technology that makes
> REST removal possible. Details, remaining work and a resume message
> are in `CLAUDE.md` — see the top "session resume note" and the HANDOFF
> entries (continued 5–10).
>
> **2026-08-31 follow-up (P3 body complete + Requirement ③ implementation
> track kicked off)**: **REST-complete-abolition (P3) is now done across
> all 5 target features** — `closed-timestamp`, `wal-service`,
> `sharded-store` (cont. 10) plus `ephemeral-query` (cont. 12) and
> `multi-raft` (cont. 13, solved by moving `EngineApplier` into
> `aruaru-dist` rather than a trait-object refactor) are all migrated to
> GraphQL, their REST routes removed, and verified end-to-end against a
> real running process. Work then moved to Requirement ③ (adopting the
> "CockroachDB × Snowflake hybrid variant" implementation techniques):
> **A.6-1 HLC** (Hybrid Logical Clock, lock-free CAS implementation),
> **A.6-2 ColumnarApplier** (the flagship item — a Raft-learner row→column
> async replica, the core of TiFlash-style HTAP; verified not just with
> unit tests but by actually running two real processes — a leader and a
> `--columnar-learner` — and confirming over real HTTP that Raft commits
> propagate into the columnar replica), and **A.6-4 stage 1 deletion
> vector** (`BlockMeta.deletion_vector`, Delta Lake-style logical
> deletion, wired into `prune_range`/`prune_equality`). See `CLAUDE.md`'s
> same-day HANDOFF entries (continued 13–18) for details.
>
> **Updated 2026-09-02 (cont. 20–23)**: **A.6-4 stage 2 base+delta
> Merge-on-Read** (`ColumnarApplier` upgraded from full rebuild to delta
> accumulation + threshold compaction; DELETE/UPDATE write the deletion
> vector), **HLC redesign** (`as_nanos()`'s `pt<<16` u64 overflow fixed
> via "plan B": physical component truncated to ~65µs granularity, logical
> counter packed into the low 16 bits; wired into the `closed_ts` path).
> Then a batch of next-phase items: `aruaru.yaml: htap` section, an
> **`htapReplicas` pruning-aware observation query**
> (`ColumnarApplier::prune_range_preview`/`prune_equality_preview`),
> **A.6-3** (`Applier::apply_at` records Raft index + MVCC commit-seq;
> `read_at_index` gates a stale read, returning 409 when behind), and
> **HLC `max_offset`** (`try_update`/`try_observe_ordinal`,
> `follower_read.max_offset_ms`). Finally, **`Query.htapReplicas` is now a
> first-class GraphQL query on the production `aruaru-server`** (not just
> the `--columnar-learner`-only `columnar_pod.rs` HTTP): with
> `aruaru.yaml: htap.columnar_replicas: true`, the server runs a
> **co-located `ColumnarApplier`** sharing the production `QueryEngine`
> and following writes via a new `QueryEngine::set_columnar_observer`
> channel. `htapReplicas` returns TiFlash `INFORMATION_SCHEMA.
> TIFLASH_REPLICA`-style `PROGRESS`/`AVAILABLE` plus a pruning preview;
> design informed by a WebSearch pass over TiFlash's columns and
> CockroachDB issue #72393. Verified end-to-end over real HTTP `/graphql`
> (release build). **cont. 24** adds `Query.htapReplicasAll` — the
> multi-table version (like `TIFLASH_REPLICA` returning one row per
> (db, table)): list the sync state of every columnar replica without
> knowing table names. HLC case-A full migration remains future work
> (`docs/HLC_TIMESTAMP_REDESIGN.md` P-HLC-3). See `CLAUDE.md` HANDOFF
> **cont. 26**: **HLC case-A full migration (P-HLC-3)**. After an
> additional primary-source pass (CockroachDB `util/hlc` keeps
> WallTime/Logical as unpacked separate fields under a Mutex; uhlc-rs
> likewise), the internal representation is now `HlcTimestamp {
> wall_nanos: u64 (no truncation), logical: u32, synthetic: bool }` — no
> shift, no packing, so the original u64 overflow is structurally
> impossible. The external u64 ordinal (`closedTsAdvance` etc.) is kept
> **backward-compatible** as the case-B 65µs projection. New API:
> `Query.hlcNow` / `now_hlc` / `uncertainty_upper`. Verified end-to-end
> over real HTTP. See `CLAUDE.md` HANDOFF entries (continued 20–26).
>
> **cont. 27**: **P-HLC-3c / 3d**. 3c rebases the `closed_ts` follower-read
> staleness check onto CockroachDB's uncertainty interval (the whole
> `[read_ts, read_ts + max_offset]` must be closed); GraphQL
> `planFollowerRead(mode: "uncertainty-safe")`. 3d **removes the last
> shift/pack/truncation from the external u64 ordinal**: `as_ordinal()` is
> now `wall_nanos + logical` (the `& !0xFFFF` bucket and the bucket branch
> in `advance_locked` are gone — bit-for-bit CockroachDB `hlc.go`
> `Now()`), so the u64 that `closedTsAdvance`/`closed_ts` carry is now
> full-precision Unix-nanosecond scale and the case-B 65µs granularity
> cost is gone. Real HTTP E2E confirms `hlcNow` returns `wallNanos ==
> ordinal` exactly.
>
> Note: `open-cuda`/`open-directx` remain out of scope for this SET
> policy for now (no HTTP surface), but this is a provisional call, not
> permanent — revisit if `open-directx`'s DirectX work matures and
> OS-level/hardware-accelerator execution paths become advantageous.

> 📌 Pending task (2026-08-06): a plan exists to incorporate Toshiba SBM / DeepSeek techniques. See [CLAUDE.md](CLAUDE.md) for details.

> **Updated 2026-08-20**: Added an optional self-update feature
> (GitHub Releases detection + health check + automatic rollback),
> **disabled by default** (`ARUARU_DB_ENABLE_SELF_UPDATE=1` to opt
> in). Verified end-to-end against a mock GitHub Releases server:
> detect → download → self-replace → `/healthz` check → rollback to
> the previous binary on failure. See the `CLAUDE.md` HANDOFF entries
> for that date for details and known limitations.

> **Updated 2026-07-25**: The dev-policy file (`CLAUDE.md`) heading was
> renamed from "Development Policy & Dev Environment Rules" to
> "Design Philosophy & Development Policy & Dev Environment Rules",
> to more clearly separate the project's design philosophy (what we
> value), development policy (how we work), and dev environment rules
> (concrete operational conventions). See `CLAUDE.md` for details.


> **The hybrid distributed database that speaks Git.**  
> CockroachDB's distributed strong consistency × Snowflake's storage/compute separation × Git-on-SQL version control — all in Pure Rust.

[![Version](https://img.shields.io/badge/version-0.5.0-orange.svg)](https://github.com/aon-co-jp/aruaru-db/releases)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/aon-co-jp/aruaru-db/actions/workflows/ci.yml/badge.svg)](https://github.com/aon-co-jp/aruaru-db/actions)
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

### Querying a past commit's state (`AS OF COMMIT`, added 2026-07-13)

The read side of the VersionLessAPI + Git version-management hybrid (endpoints
carry no version number; the data itself keeps full commit history). When a
`WHERE pk = 'value'` clause identifies a single row, appending
`AS OF COMMIT '<commit_id>'` returns that row's value **as of that commit**,
not the latest value:

```sql
INSERT INTO items (id, qty) VALUES ('sword', 1);
SELECT aruaru_commit('first grant');          -- e.g. commit_id abc123...

UPDATE items SET qty = '5' WHERE id = 'sword';
SELECT aruaru_commit('quantity bumped');

SELECT qty FROM items WHERE id = 'sword';                          -- 5 (latest)
SELECT qty FROM items WHERE id = 'sword' AS OF COMMIT 'abc123...'; -- 1 (past)
```

**Fixed 2026-09-03**: `AS OF COMMIT` now honors **column projection** like
a normal `SELECT` (previously it always returned the full row). `SELECT *`
keeps the full row; listing columns returns those columns in that order;
an unknown column errors. Both single-row and WHERE-less full-scan work.

**Client connectivity** (Java / Rust+Axum・Poem・RPoem / Python+FastAPI・
Django・Flask / PHP+Laravel / Go / Node / .NET / COBOL / IBM z/OS
mainframe, …) is in [`docs/CLIENTS.md`](docs/CLIENTS.md): aruaru-db
exposes only the two standard contracts — pgwire (:5433) and GraphQL/HTTP
(:4001) — so **every language connects with its standard PostgreSQL
driver**; no custom driver is needed. Optional thin official connectors
that wrap Git-on-SQL idiomatically live in [`clients/`](clients/)
(`aruaru-db-connector` (Rust) / `aruaru-db` (PyPI) / `@aruaru/db` (npm) /
`aruaru/db` (Composer)). **Current limitation**: result columns are all
returned as `VARCHAR` (text) for now, so read them as strings and parse
rather than using typed getters like `get::<i32>` (`docs/CLIENTS.md §5.1`).

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

**Standalone email disaster backup (added 2026-07-25)**: a last-resort
backup safety net that can be enabled with just an email address, requiring
no VPS-to-VPS distributed sync, multi-node Raft cluster setup, or ZFS
snapshot pairing. Implemented as the `disaster_email_backup` feature in
`crates/aruaru-dist`, reusing `open-raid-z`'s `EmailBackupTarget` as-is, and
configured via an admin API (`POST /admin/disaster-email-backup`,
`x-admin-token` authenticated). Verified only against a local mock SMTP
server; real SMTP / real disconnection scenarios have not been tested (see
the 2026-07-25 HANDOFF entry in `CLAUDE.md` for the honest disclosure).

**Same-day follow-up**: wired `RaftWriter::propose_and_wait` so a genuine
quorum failure (majority-commit timeout) optionally triggers
`DisasterEmailBackup` automatically. Configured via an
`Option<Arc<DisasterEmailBackup>>` (unset = fully unchanged behavior),
sent from a background `tokio::spawn` + `spawn_blocking` task so the
caller is never blocked. See the same-day follow-up HANDOFF entry in
`CLAUDE.md` for details, verification, and remaining gaps.

**Same-day follow-up (2)**: closed/advanced the 3 previously-disclosed
open gaps. (1) Verified non-blocking behavior against a genuinely slow
SMTP server (TCP connects, but EHLO/AUTH responses are delayed by
seconds), not just an unreachable one. (2) The admin API (`POST
/admin/disaster-email-backup`) now actually injects the configured
`DisasterEmailBackup` into the **live** `RaftWriter` instance serving
traffic (added a runtime setter `set_disaster_email_backup` to the
`ReplicatedWriter` trait), not just validate-and-store. (3) Audited for
`RaftNode`-direct callers bypassing `RaftWriter`: found and fixed one at
REST `/admin/cluster/propose`. The GraphQL `cluster_propose` resolver
(`crates/aruaru-graphql/src/admin_resolvers.rs`) still writes directly to
`QueryEngine`, bypassing `RaftWriter` entirely — left undocumented^Wunfixed
this pass (follow-up). See the 3rd same-day HANDOFF entry in `CLAUDE.md`.

**Self-update (added 2026-08-20)**: `aruaru-server` can optionally detect
new GitHub Releases, download and self-replace the running binary, verify
`/healthz` within a short window, and automatically roll back to the
previous binary if the health check fails. Off by default
(`ARUARU_DB_ENABLE_SELF_UPDATE=1` to enable) since this is a stateful DB
server holding real data — an unintended restart is treated as unsafe.
End-to-end verified against a local mock GitHub Releases server (success
path and rollback path); see `CLAUDE.md`'s 2026-08-20 HANDOFF entries.

---

## 🤝 Contributing

Maintained by volunteers around the world.

- **Issues**: report bugs and propose features via GitHub Issues
- Start with a **good-first-issue** label
- Please read `CONTRIBUTING.md` first
- Discord: discuss in the community channel
- When unsure about a technical choice, verify via search (both Japanese
  and English) and GitHub research rather than relying on guesses

---

## 📄 License

Apache License 2.0 — free to use commercially, modify, and redistribute.  
© 2026 aruaru-DB Contributors
