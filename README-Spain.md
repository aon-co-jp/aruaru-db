# aruaru-DB 🦀

> **La base de datos distribuida híbrida que habla Git.**  
> Consistencia fuerte distribuida de CockroachDB × separación de almacenamiento/cómputo de Snowflake × control de versiones Git-on-SQL — todo en Pure Rust.

[![Version](https://img.shields.io/badge/version-0.5.0-orange.svg)](https://github.com/aruaru-db/aruaru-db/releases)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/aruaru-db/aruaru-db/actions/workflows/ci.yml/badge.svg)](https://github.com/aruaru-db/aruaru-db/actions)
[![Discord](https://img.shields.io/badge/Discord-community-5865F2.svg)](https://discord.gg/aruaru-db)

📖 Otros idiomas: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

---

## ✨ Por qué aruaru-DB

| Característica | CockroachDB | Snowflake | **aruaru-DB** |
|---|:---:|:---:|:---:|
| Consistencia fuerte distribuida (Raft) | ✅ | ❌ | ✅ |
| Separación de almacenamiento/cómputo | ❌ | ✅ | ✅ |
| OLAP columnar (Arrow/DataFusion) | ❌ | ✅ | ✅ |
| Git-on-SQL (branch / merge / diff) | ❌ | ❌ | ✅ |
| API GraphQL sin versiones (Versionless) | ❌ | ❌ | ✅ |
| GUI de administración Tauri | ❌ | ❌ | ✅ |
| Herramientas de migración (Postgres / MySQL / CSV) | △ | △ | ✅ |
| **Totalmente OSS (Apache-2.0)** | ❌ (2024~) | ❌ | ✅ |
| Pure Rust | ❌ (Go) | ❌ | ✅ |

---

## 🏗️ Resumen de la arquitectura

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

Más detalles en [ARCHITECTURE.md](ARCHITECTURE.md) y [docs/DATABASE.md](docs/DATABASE.md).

---

## 🚀 Inicio rápido

```bash
# Iniciar el servidor (puerto PostgreSQL 5432 + GraphQL :4000)
cargo run -p aruaru-server -- --data ./data --raft-id 1

# Conectar con psql
psql -h localhost -U root -d aruaru

# Endpoint GraphQL
open http://localhost:4000/graphql
```

### GUI de administración Tauri

```bash
cd admin
npm install
npm run tauri dev
```

---

## 📦 Estructura de crates

| Crate | Función |
|---|---|
| `aruaru-core` | Motor de almacenamiento, MVCC, control de versiones Git-on-SQL |
| `aruaru-dist` | Integración con openraft, sharding por rangos, gestión de nodos |
| `aruaru-query` | Parser SQL, enrutador HTAP, integración con DataFusion |
| `aruaru-wire` | Protocolo de cable PostgreSQL (pgwire) |
| `aruaru-graphql` | GraphQL sin versiones (Versionless) + servidor HTTP Poem |
| `aruaru-registry` | Registro de bases de datos soportadas (150+), rastreo diario, adaptadores de ingesta |
| `aruaru-migrate` | Herramienta de migración Postgres / CockroachDB / Snowflake / MySQL / CSV |
| `aruaru-backup` | Copia de seguridad, restauración, recuperación en un punto en el tiempo (Parquet) |
| `aruaru-server` | Binario principal (punto de entrada integrado de todos los crates) |

---

## 🌿 Uso de Git-on-SQL

```sql
-- Crear una rama
SELECT aruaru_branch('feature/new-schema');

-- Modificar una tabla en la rama actual
ALTER TABLE users ADD COLUMN score INT DEFAULT 0;

-- Confirmar (commit)
SELECT aruaru_commit('Add score column to users');

-- Ver el historial
SELECT * FROM aruaru_log LIMIT 10;

-- diff
SELECT * FROM aruaru_diff('main', 'feature/new-schema');

-- Fusionar (merge)
SELECT aruaru_merge('feature/new-schema', 'main');
```

---

## 🔗 Proyectos relacionados

Existe una arquitectura objetivo que combina `open-web-server` con
`poem-cosmo-tauri`/`open-runo`, PostgreSQL y `open-raid-z` (revisado
2026-07-11): transporte con redundancia cuádruple (TCP-IP/UDP-IP/QUIC
(MPQUIC)/MPTCP o SCTP) y escrituras de BD con redundancia cuádruple
(PostgreSQL/aruaru-db/replicación síncrona multi-región/registro de
auditoría independiente), diseñado para evitar la pérdida de datos de
objetos de pago y datos financieros/bursátiles en juegos online 3D.
aruaru-db participa como la capa de datos distribuida Git-on-SQL y en el
modelo híbrido de API VersionLess con versionado gestionado por Git.
Actualmente solo están implementados TCP-IP/UDP-IP; el resto aún no se ha
iniciado (ver `README.md`/`CLAUDE.md` de `open-web-server` para más
detalles).

---

## 🤝 Contribuir

Mantenido por voluntarios de todo el mundo.

- **Issues**: reporta errores o propone funcionalidades en GitHub Issues
- Empieza con las etiquetas **good-first-issue**
- Por favor, lee `CONTRIBUTING.md`
- Discord: participa en el canal de la comunidad

---

## 📄 Licencia

Apache License 2.0 — libre para uso comercial, modificación y redistribución.  
© 2026 aruaru-DB Contributors
