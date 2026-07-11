# aruaru-DB 🦀

> **قاعدة البيانات الموزعة الهجينة التي تتحدث لغة Git.**  
> الاتساق القوي الموزّع من CockroachDB × الفصل بين التخزين والحوسبة من Snowflake × إدارة الإصدارات Git-on-SQL — كل ذلك بلغة Rust الخالصة.

[![Version](https://img.shields.io/badge/version-0.5.0-orange.svg)](https://github.com/aruaru-db/aruaru-db/releases)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/aruaru-db/aruaru-db/actions/workflows/ci.yml/badge.svg)](https://github.com/aruaru-db/aruaru-db/actions)
[![Discord](https://img.shields.io/badge/Discord-community-5865F2.svg)](https://discord.gg/aruaru-db)

📖 لغات أخرى: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

---

## ✨ لماذا aruaru-DB

| الميزة | CockroachDB | Snowflake | **aruaru-DB** |
|---|:---:|:---:|:---:|
| الاتساق القوي الموزّع (Raft) | ✅ | ❌ | ✅ |
| الفصل بين التخزين والحوسبة | ❌ | ✅ | ✅ |
| OLAP عمودي (Arrow/DataFusion) | ❌ | ✅ | ✅ |
| Git-on-SQL (فرع / دمج / diff) | ❌ | ❌ | ✅ |
| واجهة GraphQL بدون إصدارات (Versionless) | ❌ | ❌ | ✅ |
| واجهة إدارة Tauri | ❌ | ❌ | ✅ |
| أدوات الترحيل (Postgres / MySQL / CSV) | △ | △ | ✅ |
| **مفتوح المصدر بالكامل (Apache-2.0)** | ❌ (2024~) | ❌ | ✅ |
| Rust خالصة | ❌ (Go) | ❌ | ✅ |

---

## 🏗️ نظرة عامة على البنية

```
┌──────────────────────────────────────────────────────────┐
│  Layer 3 : Access                                        │
│  pgwire (متوافق مع PostgreSQL) │ GraphQL (Poem/async-graphql)│
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

للتفاصيل راجع [ARCHITECTURE.md](ARCHITECTURE.md) و [docs/DATABASE.md](docs/DATABASE.md).

---

## 🚀 البدء السريع

```bash
# تشغيل الخادم (منفذ PostgreSQL 5432 + GraphQL :4000)
cargo run -p aruaru-server -- --data ./data --raft-id 1

# الاتصال عبر psql
psql -h localhost -U root -d aruaru

# نقطة نهاية GraphQL
open http://localhost:4000/graphql
```

### واجهة إدارة Tauri

```bash
cd admin
npm install
npm run tauri dev
```

---

## 📦 تركيبة الحزم (Crates)

| الحزمة | الدور |
|---|---|
| `aruaru-core` | محرك التخزين، MVCC، إدارة الإصدارات Git-on-SQL |
| `aruaru-dist` | تكامل openraft، التجزئة حسب النطاقات (Range Sharding)، إدارة العقد |
| `aruaru-query` | محلل SQL، موجّه HTAP، تكامل DataFusion |
| `aruaru-wire` | بروتوكول PostgreSQL السلكي (pgwire) |
| `aruaru-graphql` | GraphQL بدون إصدارات (Versionless) + خادم HTTP قائم على Poem |
| `aruaru-registry` | سجل قواعد البيانات المدعومة (أكثر من 150)، زحف يومي، محولات الاستيعاب |
| `aruaru-migrate` | أداة ترحيل Postgres / CockroachDB / Snowflake / MySQL / CSV |
| `aruaru-backup` | النسخ الاحتياطي، الاستعادة، الاسترداد إلى نقطة زمنية (Parquet) |
| `aruaru-server` | الملف الثنائي الرئيسي (نقطة الدخول الموحدة لجميع الحزم) |

---

## 🌿 استخدام Git-on-SQL

```sql
-- إنشاء فرع
SELECT aruaru_branch('feature/new-schema');

-- تعديل جدول في الفرع الحالي
ALTER TABLE users ADD COLUMN score INT DEFAULT 0;

-- الالتزام (commit)
SELECT aruaru_commit('Add score column to users');

-- عرض السجل
SELECT * FROM aruaru_log LIMIT 10;

-- diff
SELECT * FROM aruaru_diff('main', 'feature/new-schema');

-- الدمج (merge)
SELECT aruaru_merge('feature/new-schema', 'main');
```

---

## 🔗 مشاريع ذات صلة

توجد بنية مستهدفة تجمع بين `open-web-server` و `poem-cosmo-tauri`/`open-runo`
و PostgreSQL و `open-raid-z`: إدارة هجينة تجمع بين VersionLess API والتحكم
بالإصدارات عبر Git، فوق نقل TCP-IP/UDP-IP ثلاثي التكرار، مصممة لمنع فقدان
بيانات العناصر المدفوعة والبيانات المالية/الأوراق المالية في ألعاب
الأونلاين ثلاثية الأبعاد. يشارك aruaru-db كطبقة بيانات موزعة من نوع
Git-on-SQL (راجع `CLAUDE.md` للتفاصيل).

---

## 🤝 المساهمة

يتم صيانته من قبل متطوعين حول العالم.

- **Issues**: أبلغ عن الأخطاء واقترح الميزات عبر GitHub Issues
- ابدأ بالمهام الموسومة بـ **good-first-issue**
- يرجى قراءة `CONTRIBUTING.md` أولاً
- Discord: ناقش في قناة المجتمع

---

## 📄 الترخيص

Apache License 2.0 — حر للاستخدام التجاري والتعديل وإعادة التوزيع.  
© 2026 aruaru-DB Contributors
