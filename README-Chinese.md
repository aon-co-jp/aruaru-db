# aruaru-DB 🦀

> **会说 Git 的混合型分布式数据库。**  
> CockroachDB 的分布式强一致性 × Snowflake 的存储/计算分离 × Git-on-SQL 版本管理 —— 全部用 Pure Rust 实现。

[![Version](https://img.shields.io/badge/version-0.5.0-orange.svg)](https://github.com/aruaru-db/aruaru-db/releases)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/aruaru-db/aruaru-db/actions/workflows/ci.yml/badge.svg)](https://github.com/aruaru-db/aruaru-db/actions)
[![Discord](https://img.shields.io/badge/Discord-community-5865F2.svg)](https://discord.gg/aruaru-db)

📖 其他语言: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

---

## ✨ 为什么选择 aruaru-DB

| 功能 | CockroachDB | Snowflake | **aruaru-DB** |
|------|:---:|:---:|:---:|
| 分布式强一致性 (Raft) | ✅ | ❌ | ✅ |
| 存储/计算分离 | ❌ | ✅ | ✅ |
| 列式 OLAP (Arrow/DataFusion) | ❌ | ✅ | ✅ |
| Git-on-SQL (branch / merge / diff) | ❌ | ❌ | ✅ |
| Versionless GraphQL API | ❌ | ❌ | ✅ |
| Tauri 管理 GUI | ❌ | ❌ | ✅ |
| 迁移工具 (Postgres / MySQL / CSV) | △ | △ | ✅ |
| **完全开源 (Apache-2.0)** | ❌ (2024~) | ❌ | ✅ |
| Pure Rust | ❌ (Go) | ❌ | ✅ |

---

## 🏗️ 架构概览

```
┌──────────────────────────────────────────────────────────┐
│  Layer 3 : Access                                        │
│  pgwire (兼容 PostgreSQL)  │  GraphQL (Poem/async-graphql)│
│  REST API                  │  Tauri 管理 GUI              │
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

详情请参阅 [ARCHITECTURE.md](ARCHITECTURE.md) 和 [docs/DATABASE.md](docs/DATABASE.md)。

---

## 🚀 快速开始

```bash
# 启动服务器 (PostgreSQL 端口 5432 + GraphQL :4000)
cargo run -p aruaru-server -- --data ./data --raft-id 1

# 用 psql 连接
psql -h localhost -U root -d aruaru

# GraphQL 端点
open http://localhost:4000/graphql
```

### Tauri 管理 GUI

```bash
cd admin
npm install
npm run tauri dev
```

---

## 📦 Crate 组成

| Crate | 作用 |
|---|---|
| `aruaru-core` | 存储引擎・MVCC・Git-on-SQL 版本管理 |
| `aruaru-dist` | openraft 集成・Range 分片・节点管理・Raft提交与open-raid-z快照联动(`snapshot_pairing`,2026-07-13新增) |
| `aruaru-query` | SQL 解析器・HTAP 路由・DataFusion 集成 |
| `aruaru-wire` | PostgreSQL 线协议 (pgwire) |
| `aruaru-graphql` | Versionless GraphQL + Poem HTTP 服务器 |
| `aruaru-registry` | 受支持数据库注册表 (150+ 项)・每日爬取・导入适配器 |
| `aruaru-migrate` | Postgres / CockroachDB / Snowflake / MySQL / CSV 迁移工具 |
| `aruaru-backup` | 备份・恢复・时间点恢复 (Parquet) |
| `aruaru-server` | 主二进制文件 (所有 crate 的集成入口) |

---

## 🌿 Git-on-SQL 使用方法

```sql
-- 创建分支
SELECT aruaru_branch('feature/new-schema');

-- 在当前分支修改表
ALTER TABLE users ADD COLUMN score INT DEFAULT 0;

-- 提交
SELECT aruaru_commit('Add score column to users');

-- 查看日志
SELECT * FROM aruaru_log LIMIT 10;

-- diff
SELECT * FROM aruaru_diff('main', 'feature/new-schema');

-- 合并
SELECT aruaru_merge('feature/new-schema', 'main');
```

---

## 🔗 相关项目

存在一个将 `open-web-server` 与 `poem-cosmo-tauri`/`open-runo`、PostgreSQL、
`open-raid-z` 组合起来的目标架构(2026-07-11修订):通信层采用
TCP-IP/UDP-IP/QUIC(MPQUIC)/MPTCP(或SCTP)四重冗余,数据库写入采用
PostgreSQL/aruaru-db/多区域同步复制/独立审计日志四重冗余,用于防止
3D 网络游戏中付费道具及金融/证券数据的丢失。aruaru-db 在其中承担
分布式 Git-on-SQL 数据层的角色,并参与 VersionLess API 与 Git 版本
管理的混合方案。目前仅实现了 TCP-IP/UDP-IP,其余尚未开始(详见
`open-web-server` 的 `README.md`/`CLAUDE.md`)。

---

## 🤝 参与贡献

由来自世界各地的志愿者共同维护。

- **Issues**: 请通过 GitHub Issues 报告 bug 或提出功能建议
- 建议从带有 **good-first-issue** 标签的任务开始
- 请务必阅读 `CONTRIBUTING.md`
- Discord: 欢迎在社区频道讨论

---

## 📄 许可证

Apache License 2.0 —— 可自由用于商业用途、修改和再分发。  
© 2026 aruaru-DB Contributors
