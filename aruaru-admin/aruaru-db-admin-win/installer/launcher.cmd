@echo off
REM aruaru-DB Admin ランチャ (Tauri 非依存)
REM インストール済み Microsoft Edge を --app モードで起動し、PWA をアプリ窓で開く。
REM 配信元はインストーラが %ARUARU_ADMIN_URL% に書き込む (既定 http://localhost:8787)。

set "URL=%ARUARU_ADMIN_URL%"
if "%URL%"=="" set "URL=http://localhost:8787"

REM 同梱の静的サーバ (任意): dist を配信する簡易サーバを起動したい場合はここで
REM   start "" /B aruaru-admin-serve.exe  (省略可)

where msedge >nul 2>nul
if %errorlevel%==0 (
  start "" msedge --app=%URL% --window-size=1280,820
  goto :eof
)
REM Edge が無ければ Chrome を試す
where chrome >nul 2>nul
if %errorlevel%==0 (
  start "" chrome --app=%URL% --window-size=1280,820
  goto :eof
)
REM どちらも無ければ既定ブラウザで開く
start "" %URL%
