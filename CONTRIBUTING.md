# Contributing to aruaru-DB

世界中のボランティア開発者を歓迎します！🦀

## はじめに

1. **good-first-issue** ラベルのついた Issue から始めてください
2. Issue を担当する場合はコメントで宣言してください
3. Fork → branch → PR の流れで送ってください

## ブランチ命名規則

```
feat/branch-name        # 新機能
fix/bug-description     # バグ修正
docs/section-name       # ドキュメント
refactor/area           # リファクタ
bench/target            # ベンチマーク
```

## コミットメッセージ (Conventional Commits)

```
feat(core): add prolly tree chunk splitting
fix(wire): handle empty query in extended protocol
docs(DATABASE.md): update §4b DoltgreSQL compatibility
```

## 開発環境セットアップ

```bash
# Rust stable
rustup update stable

# 依存関係の確認
cargo check --workspace

# テスト
cargo test --workspace

# フォーマット
cargo fmt --all

# Clippy
cargo clippy --all-targets -- -D warnings
```

## Tauri Admin GUI

```bash
cd admin
npm install
npm run dev          # Vite dev server
npm run tauri dev    # Tauri + Vite hot reload
```

## クレート担当エリア

| クレート | 難易度 | 担当歓迎 |
|---------|--------|---------|
| aruaru-migrate | ⭐⭐ | CSV/JSON importer 追加 |
| aruaru-graphql | ⭐⭐ | Subscription 実装 |
| aruaru-wire | ⭐⭐⭐ | pgwire Extended Query |
| aruaru-core | ⭐⭐⭐⭐ | Prolly Tree 実装 |
| aruaru-dist | ⭐⭐⭐⭐⭐ | openraft 統合 |
| admin (Tauri) | ⭐⭐ | React ページ追加 |

## PR チェックリスト

- [ ] `cargo test --workspace` がパスする
- [ ] `cargo fmt --all` 実行済み
- [ ] `cargo clippy` が warning なし
- [ ] 変更箇所に doc コメントを追加した
- [ ] 関連する Issue 番号を PR 本文に記載した

## コードオブコンダクト

- すべての人を歓迎します
- 建設的なフィードバックを心がけてください
- 言語: 日本語・英語 どちらでも OK

## ライセンス

コントリビューションは Apache License 2.0 でライセンスされます。
