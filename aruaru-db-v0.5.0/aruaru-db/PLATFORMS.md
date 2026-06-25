# aruaru-DB Platform Support Matrix
# 2026-06-21

## 🖥️ サーバ (aruaru-server) 対応プラットフォーム

### サポート方針
aruaru-DB は PostgreSQL ワイヤプロトコル互換のため、  
**既存のすべての PostgreSQL クライアントライブラリがそのまま動作する**。  
各言語専用ドライバを書く必要がない点が最大の強み。

---

## Tier 1: 完全サポート（CI でテスト済み）

| OS | アーキテクチャ | Rust ターゲット | バイナリ配布 |
|---|---|---|---|
| Windows 11 / 10 | x86_64 | `x86_64-pc-windows-msvc` | .exe NSIS / MSI |
| Windows Server 2022/2019 | x86_64 | `x86_64-pc-windows-msvc` | .exe / Docker |
| Ubuntu 22.04 LTS / 24.04 | x86_64 | `x86_64-unknown-linux-gnu` | .deb / .rpm |
| Ubuntu 22.04 LTS / 24.04 | ARM64 | `aarch64-unknown-linux-gnu` | .deb |
| macOS 14+ (Sonoma) | Apple Silicon | `aarch64-apple-darwin` | .dmg |
| macOS 14+ | x86_64 | `x86_64-apple-darwin` | .dmg |
| Docker (linux/amd64) | x86_64 | `x86_64-unknown-linux-musl` | Docker Hub |
| Docker (linux/arm64) | ARM64 | `aarch64-unknown-linux-musl` | Docker Hub |

---

## Tier 2: 動作確認済み（CI なし・手動テスト）

| OS | アーキテクチャ | Rust ターゲット | 備考 |
|---|---|---|---|
| RHEL 9 / Rocky Linux 9 | x86_64 | `x86_64-unknown-linux-gnu` | .rpm パッケージ |
| RHEL 8 | x86_64 | `x86_64-unknown-linux-gnu` | glibc 2.17+ 必須 |
| Fedora 40+ | x86_64 | `x86_64-unknown-linux-gnu` | |
| Windows 11 | ARM64 | `aarch64-pc-windows-msvc` | Surface Pro X等 |
| Android 13+ | ARM64 | `aarch64-linux-android` | NDK r27+ |
| Android 13+ | x86_64 | `x86_64-linux-android` | エミュレータ |
| Debian 12 | x86_64/ARM64 | `*-unknown-linux-gnu` | |

---

## Tier 3: ベストエフォート（Rust 公式 Tier 3 ターゲット）

| OS | アーキテクチャ | Rust ターゲット | 制約 |
|---|---|---|---|
| Oracle Solaris 11.4 | x86_64 | `x86_64-pc-solaris` | Rust Tier 3、musl 非対応 |
| Oracle Solaris 11.4 | SPARC64 | `sparcv9-sun-solaris` | Rust Tier 3、要検証 |
| FreeBSD 14+ | x86_64 | `x86_64-unknown-freebsd` | Rust Tier 2 |
| OpenBSD 7+ | x86_64 | `x86_64-unknown-openbsd` | Rust Tier 3 |
| RHEL 7 | x86_64 | `x86_64-unknown-linux-gnu` | glibc 2.17 最低版 |

---

## ⚠️ HP-UX の対応状況

> **HP-UX (IA-64 / PA-RISC) は現時点で Rust ネイティブビルドを公式サポートしていない**

| 方式 | 実現性 | 手順 |
|------|--------|------|
| ❌ Rust ネイティブビルド | 不可 | 公式ターゲット存在せず |
| ✅ **PostgreSQL ドライバ経由** | **推奨** | HP-UX 上の既存 libpq (HP 公式) がそのまま動作 |
| △ C FFI ブリッジ | 要検証 | aruaru-wire を TCP で分離し、libpq でアクセス |
| △ Docker + Linux 互換レイヤ | 参考 | HP-UX 上の Linux 互換環境経由 |

**実用的な解決策**: HP-UX アプリは既存の PostgreSQL JDBC/ODBC ドライバを使い、  
`aruaru-server` は Linux/Solaris 側に立て、ネットワーク越しに接続する。

---

## 📱 Android 対応

```bash
# Android NDK クロスコンパイル手順
rustup target add aarch64-linux-android
rustup target add x86_64-linux-android

# NDK パス設定 (Android Studio インストール前提)
export ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/27.0.12077973
export CC_aarch64_linux_android=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android34-clang
export AR_aarch64_linux_android=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar

# ビルド
cargo build --target aarch64-linux-android --release -p aruaru-server
```

Tauri 2 の Android ターゲットを利用すれば、  
管理 GUI アプリを Android 上で動かすことも可能。

---

## 🐋 Docker イメージ

```bash
# 起動
docker run -d \
  --name aruaru-db \
  -p 5432:5432 \
  -p 4000:4000 \
  -v $PWD/data:/data \
  ghcr.io/aruaru-db/aruaru-db:latest \
  --data /data

# Docker Compose (3ノードクラスタ)
docker compose -f docker/compose.cluster.yml up
```

---

## 🔌 言語ドライバ対応表

> **pgwire 互換により、既存の PostgreSQL ドライバがすべて使用可能**

| 言語 | 推奨ライブラリ | aruaru 専用機能 |
|------|--------------|----------------|
| Python | `asyncpg` / `psycopg3` | `aruaru-py` (Git-on-SQL ラッパー) |
| Node.js / TypeScript | `pg` / `postgres` | `aruaru-js` npm パッケージ |
| Java | PostgreSQL JDBC | `aruaru-java` Maven パッケージ |
| C# / .NET | `Npgsql` | `Aruaru.Client` NuGet |
| Go | `pgx/v5` | `aruaru-go` Go module |
| PHP | `pdo_pgsql` | `aruaru/php-client` Composer |
| Ruby | `pg` gem | `aruaru-ruby` gem |
| C / C++ | `libpq` | `libaruaru.h` C header |
| Kotlin | PostgreSQL JDBC | `aruaru-kotlin` |
| Swift | `PostgresNIO` | `aruaru-swift` Swift Package |
| Rust | `sqlx` / `tokio-postgres` | `aruaru-client` (ネイティブ) |
| R | `RPostgres` | 設定のみ |
| ODBC | psqlODBC | Windows/Linux ODBC |
| JDBC | PostgreSQL JDBC 42+ | Java/Scala/Kotlin |
