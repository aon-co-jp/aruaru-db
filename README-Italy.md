# aruaru-DB 🦀

> **Il database distribuito ibrido che parla Git.**  
> Coerenza forte distribuita di CockroachDB × separazione storage/calcolo di Snowflake × versionamento Git-on-SQL — tutto in Pure Rust.

[![Version](https://img.shields.io/badge/version-0.5.0-orange.svg)](https://github.com/aruaru-db/aruaru-db/releases)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/aruaru-db/aruaru-db/actions/workflows/ci.yml/badge.svg)](https://github.com/aruaru-db/aruaru-db/actions)
[![Discord](https://img.shields.io/badge/Discord-community-5865F2.svg)](https://discord.gg/aruaru-db)

📖 Altre lingue: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

---

## ✨ Perché aruaru-DB

| Funzionalità | CockroachDB | Snowflake | **aruaru-DB** |
|---|:---:|:---:|:---:|
| Coerenza forte distribuita (Raft) | ✅ | ❌ | ✅ |
| Separazione storage/calcolo | ❌ | ✅ | ✅ |
| OLAP colonnare (Arrow/DataFusion) | ❌ | ✅ | ✅ |
| Git-on-SQL (branch / merge / diff) | ❌ | ❌ | ✅ |
| API GraphQL senza versioni (Versionless) | ❌ | ❌ | ✅ |
| GUI di amministrazione Tauri | ❌ | ❌ | ✅ |
| Strumenti di migrazione (Postgres / MySQL / CSV) | △ | △ | ✅ |
| **Completamente OSS (Apache-2.0)** | ❌ (2024~) | ❌ | ✅ |
| Pure Rust | ❌ (Go) | ❌ | ✅ |

---

## 🏗️ Panoramica dell'architettura

```
┌──────────────────────────────────────────────────────────┐
│  Layer 3 : Access                                        │
│  pgwire (compatibile PostgreSQL) │ GraphQL (Poem/async-graphql)│
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

Per i dettagli vedere [ARCHITECTURE.md](ARCHITECTURE.md) e [docs/DATABASE.md](docs/DATABASE.md).

---

## 🚀 Avvio rapido

```bash
# Avviare il server (porta PostgreSQL 5432 + GraphQL :4000)
cargo run -p aruaru-server -- --data ./data --raft-id 1

# Connettersi con psql
psql -h localhost -U root -d aruaru

# Endpoint GraphQL
open http://localhost:4000/graphql
```

### GUI di amministrazione Tauri

```bash
cd admin
npm install
npm run tauri dev
```

---

## 📦 Struttura dei crate

| Crate | Ruolo |
|---|---|
| `aruaru-core` | Motore di storage, MVCC, versionamento Git-on-SQL |
| `aruaru-dist` | Integrazione openraft, sharding per intervalli, gestione dei nodi |
| `aruaru-query` | Parser SQL, router HTAP, integrazione DataFusion |
| `aruaru-wire` | Protocollo di rete PostgreSQL (pgwire) |
| `aruaru-graphql` | GraphQL senza versioni (Versionless) + server HTTP Poem |
| `aruaru-registry` | Registro dei database supportati (150+), crawl giornaliero, adattatori di ingestione |
| `aruaru-migrate` | Strumento di migrazione Postgres / CockroachDB / Snowflake / MySQL / CSV |
| `aruaru-backup` | Backup, ripristino, recupero point-in-time (Parquet) |
| `aruaru-server` | Binario principale (punto di ingresso integrato di tutti i crate) |

---

## 🌿 Uso di Git-on-SQL

```sql
-- Creare un branch
SELECT aruaru_branch('feature/new-schema');

-- Modificare una tabella sul branch corrente
ALTER TABLE users ADD COLUMN score INT DEFAULT 0;

-- Commit
SELECT aruaru_commit('Add score column to users');

-- Visualizzare il log
SELECT * FROM aruaru_log LIMIT 10;

-- diff
SELECT * FROM aruaru_diff('main', 'feature/new-schema');

-- Merge
SELECT aruaru_merge('feature/new-schema', 'main');
```

---

## 🔗 Progetti correlati

Esiste un'architettura obiettivo che combina `open-web-server` con
`poem-cosmo-tauri`/`open-runo`, PostgreSQL e `open-raid-z` (rivisto
2026-07-11): trasporto a quadrupla ridondanza (TCP-IP/UDP-IP/QUIC
(MPQUIC)/MPTCP o SCTP) e scritture DB a quadrupla ridondanza (PostgreSQL/
aruaru-db/replica sincrona multi-regione/log di audit indipendente),
pensata per evitare la perdita di dati di oggetti a pagamento e dati
finanziari/azionari nei giochi online 3D. aruaru-db vi partecipa come
livello dati distribuito Git-on-SQL e nel modello ibrido di API
VersionLess e versionamento gestito con Git. Al momento sono implementati
solo TCP-IP/UDP-IP; il resto non è ancora iniziato (dettagli in
`README.md`/`CLAUDE.md` di `open-web-server`).

---

## 🤝 Contribuire

Mantenuto da volontari di tutto il mondo.

- **Issues**: segnala bug e proponi funzionalità tramite GitHub Issues
- Inizia dalle etichette **good-first-issue**
- Leggi assolutamente `CONTRIBUTING.md`
- Discord: discuti nel canale della community

---

## 📄 Licenza

Apache License 2.0 — libero uso commerciale, modifica e redistribuzione.  
© 2026 aruaru-DB Contributors
