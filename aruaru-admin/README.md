# aruaru-admin

aruaru-DB の監視・管理アプリ。**共通コア**を **Windows デスクトップ版**と **Web版**で共有するモノレポです。

```
aruaru-admin/
├── core/                  # @aruaru/admin-core : 共通UI (React) + APIクライアント。Tauri非依存
├── aruaru-db-admin-win/   # Windows デスクトップ版 (Tauri v2 → .msi / setup.exe)
└── aruaru-db-admin-web/   # ブラウザ版 SPA
```

## アーキテクチャ

- **共通コア (`core`)** … Dashboard / コミットログ / ブランチ / クエリ / お引越し / バックアップ / 分散並列化 / 分散DB統合 / クラスタ / 対応DB の全ページと、`aruaru-server` の `/admin` REST・`/graphql` を叩く **fetch ベースの API クライアント**を持つ。Tauri ランタイムに一切依存しない。
- **invoke 互換シム** … 既存ページは `invoke("コマンド名", args)` を呼ぶが、core の `invoke` が各コマンドを REST/GraphQL の `fetch` に変換する。これにより win/web 双方が同一ページコードを共有できる。
- **win 版** … Tauri v2 のシェル。UI は core をそのまま使い、ファイルダイアログのみ Tauri プラグインを使用 (web では `prompt` にフォールバック)。
- **web 版** … core を静的 SPA としてビルド。接続先は `VITE_ARUARU_SERVER` か画面の設定 (localStorage) で解決。

```
[ aruaru-db-admin-win (Tauri) ] ─┐
                                  ├─ @aruaru/admin-core ── fetch ──> aruaru-server (:4000 /admin, /graphql)
[ aruaru-db-admin-web (SPA)   ] ─┘
```

## 開発

```bash
cd aruaru-admin
pnpm install

# Web 版 (http://localhost:5174)
pnpm web:dev

# Windows 版 (Tauri 開発ウィンドウ)
pnpm win:dev
```

事前に `aruaru-server` を起動しておくこと:
```bash
cargo run -p aruaru-server -- --gql-port 4000 --pg-port 5432 --data ./data
```

## ビルド / インストーラ生成

### Windows インストーラ (.msi + setup.exe)
```bash
cd aruaru-admin
pnpm install
# アイコン生成 (初回。1024x1024 PNG から)
pnpm --filter aruaru-db-admin-win exec tauri icon path/to/logo.png
# .msi と setup.exe の両方を生成
pnpm win:build
# 出力:
#   aruaru-db-admin-win/src-tauri/target/release/bundle/msi/*.msi
#   aruaru-db-admin-win/src-tauri/target/release/bundle/nsis/*.exe
```
`tauri.conf.json` の `bundle.targets: ["msi", "nsis"]` で両形式を生成します。

### Web 版
```bash
cd aruaru-admin
VITE_ARUARU_SERVER=https://db.example.com pnpm web:build
# 出力: aruaru-db-admin-web/dist  (任意の静的ホスティング / 同梱 Dockerfile で nginx 配信)
```

## CI

`.github/workflows/admin-build.yml` が `admin-v*` タグ push で起動し、
Windows ランナーで `.msi`/`setup.exe` を、Ubuntu で Web バンドルを生成してアーティファクト化します。

## 注意 (現状)

- Windows ビルドには Windows + Rust + Node が必要です (WiX は Tauri が取得)。`.msi`/`.exe` の生成はこのリポジトリ単体ではなく、お手元か CI で行います。
- 接続先 `aruaru-server` 側は Web 版 (別オリジン) のアクセスを許可する CORS を有効化済みです。
- アイコンは未同梱です。`tauri icon` で生成してください (`src-tauri/icons/README.md` 参照)。
