# GitHub へのアップロード手順
# ── aruaru-DB を自分の GitHub にアップする ──

## ⚠️ 重要: Claude は GitHub に直接アップできません

Claude が生成したファイルは ZIP でダウンロードしてください。  
GitHub への push は以下の手順でご自身の端末から行います。

---

## 手順 1: ZIP を展開

```bash
# ダウンロードした ZIP を展開
unzip aruaru-db.zip
cd aruaru-db
```

---

## 手順 2: GitHub にリポジトリを作成

1. https://github.com/new を開く
2. Repository name: `aruaru-db`
3. Description: `Hybrid distributed database: CockroachDB × Snowflake in Pure Rust`
4. Public ✅ (世界中のボランティアに参加してもらうため)
5. **Initialize this repository はチェックしない** (既にファイルがあるため)
6. Create repository

---

## 手順 3: git 初期化 & push

```bash
cd aruaru-db

# Git 初期化
git init
git add .
git commit -m "feat: aruaru-DB v0.3.0

- Pure Rust workspace (7 crates)
- CockroachDB × Snowflake hybrid architecture
- Git-on-SQL version control (branch/commit/diff)
- Versionless GraphQL API (Poem + async-graphql)
- pgwire PostgreSQL wire protocol
- Tauri 2 Admin GUI (backup/cluster/migration)
- Language drivers: Python/Node.js/Java/.NET/Go/PHP/Ruby/C++
- Platform support: Windows/Linux/macOS/Android (PLATFORMS.md)
- Docker multi-arch + 3-node cluster compose
- CI: cross-compile all targets"

# GitHub リモートを追加 (YOUR_GITHUB_USERNAME を自分のIDに変更)
git remote add origin https://github.com/YOUR_GITHUB_USERNAME/aruaru-db.git

# push
git branch -M main
git push -u origin main
```

---

## 手順 4: GitHub リポジトリ設定

### Topics (検索で見つかりやすくなる)
Settings → Topics に以下を追加:
```
rust database distributed-database git-version-control
graphql postgresql-compatible tauri htap olap oltp
cockroachdb snowflake open-source apache-license
```

### Branch protection
Settings → Branches → Add rule:
- Branch name: `main`
- Require a pull request before merging ✅
- Require status checks to pass ✅ (CI)

### GitHub Pages (ドキュメントサイト)
Settings → Pages → Source: `docs/` フォルダ (後で整備)

---

## 手順 5: Issues / Labels 設定

以下の Labels を作成:
```
good first issue    (緑) - 初心者向け
help wanted        (青) - コントリビュート募集
core/storage       (赤) - aruaru-core 関連
core/dist          (紫) - 分散・Raft
feature/graphql    (黄) - GraphQL API
feature/tauri      (橙) - 管理GUI
driver/python      (緑) - Python ドライバ
driver/nodejs      (緑) - Node.js ドライバ
driver/java        (緑) - Java ドライバ
platform/windows   (青) - Windows対応
platform/android   (青) - Android対応
platform/solaris   (灰) - Solaris対応
bug                (赤) - バグ
docs               (白) - ドキュメント
```

---

## 手順 6: 最初の Issues を作成

世界中のコントリビュータを集めるため、以下の Issues を作成:

```markdown
# good first issue の例

## [good first issue] CSV importer の完成
crates/aruaru-migrate/src/from_csv.rs の TODO を実装する
Arrow CSV reader を使って CSV を行に変換し、
aruaru-wire 経由で insert するコードを書いてください

## [good first issue] Python ドライバの pip パッケージ化
drivers/python/ を正式な PyPI パッケージにする
setup.py / pyproject.toml + README

## [help wanted] pgwire SimpleQueryHandler 実装
crates/aruaru-wire/src/lib.rs の TODO を実装する
pgwire crate の SimpleQueryHandler trait を
aruaru-query に接続してください
難易度: ⭐⭐⭐
```

---

## 手順 7: 定期アップデート

```bash
# 新しい変更を Claude に生成させた後
cd aruaru-db
git add .
git commit -m "feat(graphql): add Subscription for real-time change streaming"
git push origin main
```

---

## 推奨: Organization として運営

個人アカウントではなく GitHub Organization として運営すると:
- 複数の管理者を設定できる
- リポジトリをグループ化できる
- aruaru-db organization → aruaru-db/aruaru-db

作成: https://github.com/organizations/new
Organization name: `aruaru-db`
