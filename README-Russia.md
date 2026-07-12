# aruaru-DB 🦀

> **Гибридная распределённая база данных, говорящая на языке Git.**  
> Распределённая строгая согласованность CockroachDB × разделение хранения и вычислений Snowflake × версионирование Git-on-SQL — всё на Pure Rust.

[![Version](https://img.shields.io/badge/version-0.5.0-orange.svg)](https://github.com/aruaru-db/aruaru-db/releases)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/aruaru-db/aruaru-db/actions/workflows/ci.yml/badge.svg)](https://github.com/aruaru-db/aruaru-db/actions)
[![Discord](https://img.shields.io/badge/Discord-community-5865F2.svg)](https://discord.gg/aruaru-db)

📖 Другие языки: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

---

## ✨ Почему aruaru-DB

| Возможность | CockroachDB | Snowflake | **aruaru-DB** |
|---|:---:|:---:|:---:|
| Распределённая строгая согласованность (Raft) | ✅ | ❌ | ✅ |
| Разделение хранения и вычислений | ❌ | ✅ | ✅ |
| Колоночный OLAP (Arrow/DataFusion) | ❌ | ✅ | ✅ |
| Git-on-SQL (branch / merge / diff) | ❌ | ❌ | ✅ |
| Versionless GraphQL API | ❌ | ❌ | ✅ |
| Административный GUI на Tauri | ❌ | ❌ | ✅ |
| Инструменты миграции (Postgres / MySQL / CSV) | △ | △ | ✅ |
| **Полностью OSS (Apache-2.0)** | ❌ (2024~) | ❌ | ✅ |
| Pure Rust | ❌ (Go) | ❌ | ✅ |

---

## 🏗️ Обзор архитектуры

```
┌──────────────────────────────────────────────────────────┐
│  Layer 3 : Access                                        │
│  pgwire (совместимо с PostgreSQL) │ GraphQL (Poem/async-graphql)│
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

Подробности см. в [ARCHITECTURE.md](ARCHITECTURE.md) и [docs/DATABASE.md](docs/DATABASE.md).

---

## 🚀 Быстрый старт

```bash
# Запуск сервера (порт PostgreSQL 5432 + GraphQL :4000)
cargo run -p aruaru-server -- --data ./data --raft-id 1

# Подключение через psql
psql -h localhost -U root -d aruaru

# GraphQL-эндпоинт
open http://localhost:4000/graphql
```

### Административный GUI на Tauri

```bash
cd admin
npm install
npm run tauri dev
```

---

## 📦 Состав крейтов

| Крейт | Роль |
|---|---|
| `aruaru-core` | Движок хранения, MVCC, версионирование Git-on-SQL |
| `aruaru-dist` | Интеграция openraft, шардирование по диапазонам, управление узлами, связка commit Raft со снапшотами open-raid-z (`snapshot_pairing`, добавлено 2026-07-13) |
| `aruaru-query` | SQL-парсер, HTAP-роутер, интеграция с DataFusion |
| `aruaru-wire` | Проводной протокол PostgreSQL (pgwire) |
| `aruaru-graphql` | Versionless GraphQL + HTTP-сервер на Poem |
| `aruaru-registry` | Реестр поддерживаемых БД (150+), ежедневный краулинг, адаптеры импорта |
| `aruaru-migrate` | Инструмент миграции Postgres / CockroachDB / Snowflake / MySQL / CSV |
| `aruaru-backup` | Резервное копирование, восстановление, point-in-time recovery (Parquet) |
| `aruaru-server` | Основной бинарный файл (единая точка входа для всех крейтов) |

---

## 🌿 Использование Git-on-SQL

```sql
-- Создать ветку
SELECT aruaru_branch('feature/new-schema');

-- Изменить таблицу в текущей ветке
ALTER TABLE users ADD COLUMN score INT DEFAULT 0;

-- Commit
SELECT aruaru_commit('Add score column to users');

-- Посмотреть журнал
SELECT * FROM aruaru_log LIMIT 10;

-- diff
SELECT * FROM aruaru_diff('main', 'feature/new-schema');

-- Merge
SELECT aruaru_merge('feature/new-schema', 'main');
```

> **Новое (2026-07-13)**: поддерживается `SELECT col FROM t WHERE pk = 'v'
> AS OF COMMIT '<commit_id>'` — возвращает значение строки на момент
> указанного прошлого коммита (по PK), а не самое свежее значение (только
> запросы по одной строке; полное сканирование таблицы пока не
> поддерживается; ещё не доступно через pgwire для внешних вызывающих).

---

## 🔗 Связанные проекты

Существует целевая архитектура, объединяющая `open-web-server` с
`poem-cosmo-tauri`/`open-runo`, PostgreSQL и `open-raid-z` (пересмотрено
2026-07-11): четырёхкратно резервированный транспорт (TCP-IP/UDP-IP/QUIC
(MPQUIC)/MPTCP или SCTP) и четырёхкратно резервированная запись в БД
(PostgreSQL/aruaru-db/синхронная мультирегиональная репликация/
независимый журнал аудита), призванная предотвратить потерю данных
платных предметов и финансовых/биржевых данных в 3D онлайн-играх.
aruaru-db выступает в ней как распределённый слой данных Git-on-SQL и
участвует в гибридной модели VersionLess API и версионирования на основе
Git. На данный момент реализованы только TCP-IP/UDP-IP, остальное ещё не
начато (подробности см. в `README.md`/`CLAUDE.md` `open-web-server`).

---

## 🤝 Участие в разработке

Поддерживается волонтёрами со всего мира.

- **Issues**: сообщайте об ошибках и предлагайте функции через GitHub Issues
- Начните с задач с меткой **good-first-issue**
- Обязательно прочитайте `CONTRIBUTING.md`
- Discord: обсуждение в канале сообщества

---

## 📄 Лицензия

Apache License 2.0 — свободное коммерческое использование, изменение и распространение.  
© 2026 aruaru-DB Contributors
