# aruaru-db インストールスクリプト(Windows / Windows Server 共通)。
#
# 使い方(管理者権限のPowerShellで、zipを手動展開せずそのまま実行できる):
#   Invoke-WebRequest -Uri "https://raw.githubusercontent.com/aon-co-jp/aruaru-db/main/install.ps1" -OutFile install.ps1
#   .\install.ps1
#
# 実バグ修正・簡易化(2026-08-12、ユーザー指示「aruaru-dbやaruaru-llm
# などのインストールもより簡単にして」への対応、`open-english`の
# 「Setup aruaru-db」導線からの実地調査で発覚):
# 1. **URLの誤り**: これまで本コメント・README.md・install.shが参照
#    していたGitHubリポジトリURLは`aruaru-db/aruaru-db`という誤った
#    組織名で、実際のリポジトリ(`aon-co-jp/aruaru-db`、`git remote -v`
#    で確認)とは異なっていた——手順通りに実行すると404になる状態が
#    ここまで放置されていた。
# 2. **手動手順が多すぎた**: 従来はzipを手動でダウンロード・展開して
#    このスクリプトを実行し、さらにサービス登録は3行のコマンドを
#    このスクリプトが**印刷するだけ**で、ユーザー自身がコピペして
#    実行する必要があった。今回、(a)`aruaru-server.exe`が見つからない
#    場合はGitHub Releases APIから最新のWindows向けzipを自動取得・
#    自動展開し、(b)サービス未登録の場合は`New-Service`/`Start-Service`
#    まで自動実行するよう変更した(`-SkipServiceRegistration`スイッチ
#    で従来通り印刷のみに戻すことも可能)。

#Requires -RunAsAdministrator

param(
    [switch]$SkipServiceRegistration
)

$ErrorActionPreference = "Stop"

$InstallDir = "C:\Program Files\aruaru-db"
$DataDir = "C:\ProgramData\aruaru-db"
$ServiceName = "AruaruDb"

Write-Host "==> インストール先: $InstallDir"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
New-Item -ItemType Directory -Force -Path $DataDir | Out-Null

$BinSrc = Join-Path $PSScriptRoot "aruaru-server.exe"
if (-not (Test-Path $BinSrc)) {
    Write-Host "==> aruaru-server.exe が見つからないため、GitHub Releasesから最新版を自動取得します"
    $apiUrl = "https://api.github.com/repos/aon-co-jp/aruaru-db/releases/latest"
    $release = Invoke-RestMethod -Uri $apiUrl -Headers @{ "User-Agent" = "aruaru-db-installer" }
    $asset = $release.assets | Where-Object { $_.name -like "*windows*x86_64*.zip" } | Select-Object -First 1
    if (-not $asset) {
        Write-Error "Windows向けリリースアセットが見つかりませんでした。https://github.com/aon-co-jp/aruaru-db/releases から手動で取得してください。"
        exit 1
    }
    $zipPath = Join-Path $PSScriptRoot "aruaru-db-download.zip"
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath -UseBasicParsing
    Expand-Archive -Path $zipPath -DestinationPath $PSScriptRoot -Force
    Remove-Item $zipPath -Force
    if (-not (Test-Path $BinSrc)) {
        Write-Error "ダウンロード・展開後もaruaru-server.exe が見つかりません($BinSrc)。zipの内部構成が変わった可能性があります。"
        exit 1
    }
}
Copy-Item $BinSrc -Destination $InstallDir -Force

$existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "==> 既存のWindowsサービスが見つかったため、バイナリのみ更新しました(再起動は行いません)"
    Write-Host "    手動で再起動する場合: Restart-Service $ServiceName"
} elseif ($SkipServiceRegistration) {
    Write-Host "==> -SkipServiceRegistration指定のため、Windowsサービスとしての登録はスキップしました。手動で登録する場合の手順:"
    Write-Host "      [Environment]::SetEnvironmentVariable('ARUARU_DATA_DIR', '$DataDir', 'Machine')"
    Write-Host "      New-Service -Name $ServiceName -BinaryPathName '$InstallDir\aruaru-server.exe' -DisplayName 'aruaru-db' -StartupType Automatic"
    Write-Host "      Start-Service $ServiceName"
} else {
    Write-Host "==> Windowsサービスとして自動登録します"
    [Environment]::SetEnvironmentVariable('ARUARU_DATA_DIR', $DataDir, 'Machine')
    New-Service -Name $ServiceName -BinaryPathName "$InstallDir\aruaru-server.exe" -DisplayName "aruaru-db" -StartupType Automatic | Out-Null
    Start-Service -Name $ServiceName
    Write-Host "==> サービス '$ServiceName' を登録・起動しました(Get-Service $ServiceName で確認できます)"
}

Write-Host "==> 完了。"
