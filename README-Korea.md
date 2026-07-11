# aruaru-DB 🦀

> **Git를 말하는 하이브리드 분산 데이터베이스.**  
> CockroachDB의 분산 강일관성 × Snowflake의 스토리지/컴퓨트 분리 × Git-on-SQL 버전 관리 —— 모두 Pure Rust로.

[![Version](https://img.shields.io/badge/version-0.5.0-orange.svg)](https://github.com/aruaru-db/aruaru-db/releases)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/aruaru-db/aruaru-db/actions/workflows/ci.yml/badge.svg)](https://github.com/aruaru-db/aruaru-db/actions)
[![Discord](https://img.shields.io/badge/Discord-community-5865F2.svg)](https://discord.gg/aruaru-db)

📖 다른 언어: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

---

## ✨ 왜 aruaru-DB인가

| 기능 | CockroachDB | Snowflake | **aruaru-DB** |
|------|:---:|:---:|:---:|
| 분산 강일관성 (Raft) | ✅ | ❌ | ✅ |
| 스토리지/컴퓨트 분리 | ❌ | ✅ | ✅ |
| 컬럼형 OLAP (Arrow/DataFusion) | ❌ | ✅ | ✅ |
| Git-on-SQL (branch / merge / diff) | ❌ | ❌ | ✅ |
| Versionless GraphQL API | ❌ | ❌ | ✅ |
| Tauri 관리 GUI | ❌ | ❌ | ✅ |
| 마이그레이션 도구 (Postgres / MySQL / CSV) | △ | △ | ✅ |
| **완전 OSS (Apache-2.0)** | ❌ (2024~) | ❌ | ✅ |
| Pure Rust | ❌ (Go) | ❌ | ✅ |

---

## 🏗️ 아키텍처 개요

```
┌──────────────────────────────────────────────────────────┐
│  Layer 3 : Access                                        │
│  pgwire (PostgreSQL 호환)  │  GraphQL (Poem/async-graphql)│
│  REST API                  │  Tauri 관리 GUI              │
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

자세한 내용은 [ARCHITECTURE.md](ARCHITECTURE.md)와 [docs/DATABASE.md](docs/DATABASE.md)를 참고하세요.

---

## 🚀 빠른 시작

```bash
# 서버 시작 (PostgreSQL 포트 5432 + GraphQL :4000)
cargo run -p aruaru-server -- --data ./data --raft-id 1

# psql로 접속
psql -h localhost -U root -d aruaru

# GraphQL 엔드포인트
open http://localhost:4000/graphql
```

### Tauri 관리 GUI

```bash
cd admin
npm install
npm run tauri dev
```

---

## 📦 크레이트 구성

| 크레이트 | 역할 |
|---|---|
| `aruaru-core` | 스토리지 엔진・MVCC・Git-on-SQL 버전 관리 |
| `aruaru-dist` | openraft 통합・Range 샤딩・노드 관리 |
| `aruaru-query` | SQL 파서・HTAP 라우터・DataFusion 통합 |
| `aruaru-wire` | PostgreSQL 와이어 프로토콜 (pgwire) |
| `aruaru-graphql` | Versionless GraphQL + Poem HTTP 서버 |
| `aruaru-registry` | 지원 DB 레지스트리 (150개 이상)・매일 크롤링・수집 어댑터 |
| `aruaru-migrate` | Postgres / CockroachDB / Snowflake / MySQL / CSV 마이그레이션 도구 |
| `aruaru-backup` | 백업・복원・시점 복구 (Parquet) |
| `aruaru-server` | 메인 바이너리 (모든 크레이트의 통합 엔트리 포인트) |

---

## 🌿 Git-on-SQL 사용법

```sql
-- 브랜치 생성
SELECT aruaru_branch('feature/new-schema');

-- 현재 브랜치에서 테이블 변경
ALTER TABLE users ADD COLUMN score INT DEFAULT 0;

-- 커밋
SELECT aruaru_commit('Add score column to users');

-- 로그 확인
SELECT * FROM aruaru_log LIMIT 10;

-- diff
SELECT * FROM aruaru_diff('main', 'feature/new-schema');

-- 머지
SELECT aruaru_merge('feature/new-schema', 'main');
```

---

## 🔗 관련 프로젝트

`open-web-server`를 중심으로 `poem-cosmo-tauri`/`open-runo`, PostgreSQL,
`open-raid-z`를 결합한 목표 아키텍처가 있다: 3중 이중화된 TCP-IP/UDP-IP
전송 위에서 VersionLess API와 Git 기반 버전 관리를 결합해, 3D 온라인
게임의 유료 아이템 및 금융/증권 데이터 손실을 방지하는 구조다. aruaru-db는
그 안에서 분산 Git-on-SQL 데이터 계층 역할을 담당한다(자세한 내용은
`CLAUDE.md` 참조).

---

## 🤝 기여하기

전 세계 자원봉사자들이 유지보수하고 있습니다.

- **Issues**: 버그 신고와 기능 제안은 GitHub Issues로
- **good-first-issue** 라벨부터 시작해보세요
- 반드시 `CONTRIBUTING.md`를 읽어주세요
- Discord: 커뮤니티 채널에서 논의

---

## 📄 라이선스

Apache License 2.0 —— 상업적 이용・수정・재배포 모두 자유.  
© 2026 aruaru-DB Contributors
