# aruaru-DB 🦀

> **Die hybride verteilte Datenbank, die Git spricht.**  
> CockroachDBs verteilte starke Konsistenz × Snowflakes Trennung von Speicher und Compute × Git-on-SQL-Versionsverwaltung — alles in Pure Rust.

[![Version](https://img.shields.io/badge/version-0.5.0-orange.svg)](https://github.com/aruaru-db/aruaru-db/releases)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/aruaru-db/aruaru-db/actions/workflows/ci.yml/badge.svg)](https://github.com/aruaru-db/aruaru-db/actions)
[![Discord](https://img.shields.io/badge/Discord-community-5865F2.svg)](https://discord.gg/aruaru-db)

📖 Andere Sprachen: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

---

## ✨ Warum aruaru-DB

| Funktion | CockroachDB | Snowflake | **aruaru-DB** |
|---|:---:|:---:|:---:|
| Verteilte starke Konsistenz (Raft) | ✅ | ❌ | ✅ |
| Trennung von Speicher und Compute | ❌ | ✅ | ✅ |
| Spaltenorientiertes OLAP (Arrow/DataFusion) | ❌ | ✅ | ✅ |
| Git-on-SQL (branch / merge / diff) | ❌ | ❌ | ✅ |
| Versionless GraphQL API | ❌ | ❌ | ✅ |
| Tauri Admin-GUI | ❌ | ❌ | ✅ |
| Migrationswerkzeuge (Postgres / MySQL / CSV) | △ | △ | ✅ |
| **Vollständig OSS (Apache-2.0)** | ❌ (2024~) | ❌ | ✅ |
| Pure Rust | ❌ (Go) | ❌ | ✅ |

---

## 🏗️ Architekturübersicht

```
┌──────────────────────────────────────────────────────────┐
│  Layer 3 : Access                                        │
│  pgwire (PostgreSQL-kompatibel) │ GraphQL (Poem/async-graphql)│
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

Details siehe [ARCHITECTURE.md](ARCHITECTURE.md) und [docs/DATABASE.md](docs/DATABASE.md).

---

## 🚀 Schnellstart

```bash
# Server starten (PostgreSQL-Port 5432 + GraphQL :4000)
cargo run -p aruaru-server -- --data ./data --raft-id 1

# Mit psql verbinden
psql -h localhost -U root -d aruaru

# GraphQL-Endpunkt
open http://localhost:4000/graphql
```

### Tauri Admin-GUI

```bash
cd admin
npm install
npm run tauri dev
```

---

## 📦 Crate-Aufbau

| Crate | Aufgabe |
|---|---|
| `aruaru-core` | Storage-Engine, MVCC, Git-on-SQL-Versionsverwaltung |
| `aruaru-dist` | openraft-Integration, Range-Sharding, Knotenverwaltung |
| `aruaru-query` | SQL-Parser, HTAP-Router, DataFusion-Integration |
| `aruaru-wire` | PostgreSQL-Wire-Protokoll (pgwire) |
| `aruaru-graphql` | Versionless GraphQL + Poem-HTTP-Server |
| `aruaru-registry` | Registry unterstützter Datenbanken (150+), täglicher Crawl, Ingest-Adapter |
| `aruaru-migrate` | Migrationswerkzeug für Postgres / CockroachDB / Snowflake / MySQL / CSV |
| `aruaru-backup` | Backup, Restore, Point-in-Time-Recovery (Parquet) |
| `aruaru-server` | Haupt-Binary (integrierter Einstiegspunkt aller Crates) |

---

## 🌿 Git-on-SQL verwenden

```sql
-- Branch erstellen
SELECT aruaru_branch('feature/new-schema');

-- Tabelle im aktuellen Branch ändern
ALTER TABLE users ADD COLUMN score INT DEFAULT 0;

-- Commit
SELECT aruaru_commit('Add score column to users');

-- Log ansehen
SELECT * FROM aruaru_log LIMIT 10;

-- diff
SELECT * FROM aruaru_diff('main', 'feature/new-schema');

-- Merge
SELECT aruaru_merge('feature/new-schema', 'main');
```

---

## 🔗 Verwandte Projekte

Es gibt eine Zielarchitektur, die `open-web-server` mit
`poem-cosmo-tauri`/`open-runo`, PostgreSQL und `open-raid-z` kombiniert
(überarbeitet 2026-07-11): vierfach redundanter Transport (TCP-IP/UDP-IP/
QUIC (MPQUIC)/MPTCP oder SCTP) und vierfach redundante DB-Schreibvorgänge
(PostgreSQL/aruaru-db/synchrone Multi-Region-Replikation/unabhängiges
Audit-Log), die den Verlust von Bezahlobjekten sowie Finanz-/
Wertpapierdaten in 3D-Onlinespielen verhindern sollen. aruaru-db fungiert
dabei als verteilte Git-on-SQL-Datenschicht und ist Teil des hybriden
Modells aus VersionLess API und Git-verwalteter Versionierung. Aktuell
sind nur TCP-IP/UDP-IP implementiert; der Rest ist noch offen (Details
siehe `README.md`/`CLAUDE.md` von `open-web-server`).

---

## 🤝 Mitwirken

Wird von Freiwilligen aus aller Welt gepflegt.

- **Issues**: Fehlerberichte und Feature-Vorschläge über GitHub Issues
- Am besten mit dem Label **good-first-issue** starten
- Bitte unbedingt `CONTRIBUTING.md` lesen
- Discord: Diskussion im Community-Channel

---

## 📄 Lizenz

Apache License 2.0 — frei für kommerzielle Nutzung, Änderung und Weitergabe.  
© 2026 aruaru-DB Contributors
