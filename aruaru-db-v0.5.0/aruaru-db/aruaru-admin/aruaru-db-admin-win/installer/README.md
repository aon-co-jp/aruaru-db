# aruaru-DB Admin — Windows インストーラビルド手順

## 前提ツール

| ツール | 用途 | 入手先 |
|--------|------|--------|
| Node.js 20+ / pnpm | Web ビルド | https://pnpm.io |
| NSIS 3.x | .exe 生成 | https://nsis.sf.net |
| WiX Toolset v4 | .msi 生成 | https://wixtoolset.org |

## ビルド手順

```bat
rem 1. Web アセットをビルド
cd aruaru-admin
pnpm install
pnpm --filter aruaru-db-admin-web build

rem 2. dist/ を installer/web-dist/ にコピー
xcopy /E /I aruaru-db-admin-web\dist installer\web-dist\

rem 3-a. NSIS で .exe 生成
cd installer
makensis setup.nsi
rem → aruaru-db-admin-setup-0.5.0.exe が生成される

rem 3-b. WiX v4 で .msi 生成
wix harvest web-dist -cg WebAssets -dr INSTALLDIR -o web-assets.wxs
wix build setup.wxs web-assets.wxs -out aruaru-db-admin-0.5.0.msi
rem → aruaru-db-admin-0.5.0.msi が生成される
```

## インストール後の動作

1. スタートメニュー / デスクトップの "aruaru-DB Admin" を起動
2. Edge (WebView2) でアプリモード起動 → PWA として動作
3. Edge がなければ既定ブラウザで `file:///.../index.html` を開く
4. 接続先は画面右上「設定」で変更可能
   - 既定: `http://localhost:4000/graphql`
   - 本番: `https://api.example.com/graphql`

## 将来の Hive Gateway 差し込み

インストーラの変更は不要。
`VITE_ARUARU_GQL_ENDPOINT=https://hive.example.com/graphql pnpm build` で
ビルドし直すだけで、Hive Gateway 経由の成果物になります。
