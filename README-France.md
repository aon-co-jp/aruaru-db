# aruaru-DB 🦀

> **La base de données distribuée hybride qui parle Git.**  
> Cohérence forte distribuée de CockroachDB × séparation stockage/calcul de Snowflake × gestion de versions Git-on-SQL — le tout en Pure Rust.

[![Version](https://img.shields.io/badge/version-0.5.0-orange.svg)](https://github.com/aruaru-db/aruaru-db/releases)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/aruaru-db/aruaru-db/actions/workflows/ci.yml/badge.svg)](https://github.com/aruaru-db/aruaru-db/actions)
[![Discord](https://img.shields.io/badge/Discord-community-5865F2.svg)](https://discord.gg/aruaru-db)

📖 Autres langues : [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

---

## ✨ Pourquoi aruaru-DB

| Fonctionnalité | CockroachDB | Snowflake | **aruaru-DB** |
|---|:---:|:---:|:---:|
| Cohérence forte distribuée (Raft) | ✅ | ❌ | ✅ |
| Séparation stockage/calcul | ❌ | ✅ | ✅ |
| OLAP colonnaire (Arrow/DataFusion) | ❌ | ✅ | ✅ |
| Git-on-SQL (branch / merge / diff) | ❌ | ❌ | ✅ |
| API GraphQL sans versions (Versionless) | ❌ | ❌ | ✅ |
| GUI d'administration Tauri | ❌ | ❌ | ✅ |
| Outils de migration (Postgres / MySQL / CSV) | △ | △ | ✅ |
| **Entièrement OSS (Apache-2.0)** | ❌ (2024~) | ❌ | ✅ |
| Pure Rust | ❌ (Go) | ❌ | ✅ |

---

## 🏗️ Aperçu de l'architecture

```
┌──────────────────────────────────────────────────────────┐
│  Layer 3 : Access                                        │
│  pgwire (compatible PostgreSQL) │ GraphQL (Poem/async-graphql)│
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

Voir [ARCHITECTURE.md](ARCHITECTURE.md) et [docs/DATABASE.md](docs/DATABASE.md) pour plus de détails.

---

## 🚀 Démarrage rapide

```bash
# Démarrer le serveur (port PostgreSQL 5432 + GraphQL :4000)
cargo run -p aruaru-server -- --data ./data --raft-id 1

# Se connecter avec psql
psql -h localhost -U root -d aruaru

# Point d'accès GraphQL
open http://localhost:4000/graphql
```

### GUI d'administration Tauri

```bash
cd admin
npm install
npm run tauri dev
```

---

## 📦 Organisation des crates

| Crate | Rôle |
|---|---|
| `aruaru-core` | Moteur de stockage, MVCC, gestion de versions Git-on-SQL |
| `aruaru-dist` | Intégration openraft, sharding par plages, gestion des nœuds |
| `aruaru-query` | Parseur SQL, routeur HTAP, intégration DataFusion |
| `aruaru-wire` | Protocole de câble PostgreSQL (pgwire) |
| `aruaru-graphql` | GraphQL sans versions (Versionless) + serveur HTTP Poem |
| `aruaru-registry` | Registre des bases de données prises en charge (150+), crawl quotidien, adaptateurs d'ingestion |
| `aruaru-migrate` | Outil de migration Postgres / CockroachDB / Snowflake / MySQL / CSV |
| `aruaru-backup` | Sauvegarde, restauration, récupération à un point dans le temps (Parquet) |
| `aruaru-server` | Binaire principal (point d'entrée intégré de tous les crates) |

---

## 🌿 Utilisation de Git-on-SQL

```sql
-- Créer une branche
SELECT aruaru_branch('feature/new-schema');

-- Modifier une table sur la branche courante
ALTER TABLE users ADD COLUMN score INT DEFAULT 0;

-- Valider (commit)
SELECT aruaru_commit('Add score column to users');

-- Consulter l'historique
SELECT * FROM aruaru_log LIMIT 10;

-- diff
SELECT * FROM aruaru_diff('main', 'feature/new-schema');

-- Fusionner (merge)
SELECT aruaru_merge('feature/new-schema', 'main');
```

---

## 🔗 Projets liés

Il existe une architecture cible combinant `open-web-server` avec
`poem-cosmo-tauri`/`open-runo`, PostgreSQL et `open-raid-z` (révisé le
2026-07-11) : transport quadruple-redondant (TCP-IP/UDP-IP/QUIC (MPQUIC)/
MPTCP ou SCTP) et écritures DB quadruple-redondantes (PostgreSQL/aruaru-db/
réplication synchrone multi-région/journal d'audit indépendant), conçue
pour éviter la perte des données d'objets payants et des données
financières/boursières dans les jeux en ligne 3D. aruaru-db y intervient
comme couche de données distribuée Git-on-SQL et participe au modèle
hybride API VersionLess et versionnement géré par Git. Actuellement, seuls
TCP-IP/UDP-IP sont implémentés ; le reste reste à faire (voir
`README.md`/`CLAUDE.md` de `open-web-server` pour les détails).

---

## 🤝 Contribuer

Maintenu par des bénévoles du monde entier.

- **Issues** : signalez les bugs et proposez des fonctionnalités via GitHub Issues
- Commencez par les tickets étiquetés **good-first-issue**
- Merci de lire `CONTRIBUTING.md`
- Discord : discutez sur le salon de la communauté

---

## 📄 Licence

Apache License 2.0 — libre pour un usage commercial, la modification et la redistribution.  
© 2026 aruaru-DB Contributors
