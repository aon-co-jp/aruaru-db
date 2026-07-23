# aruaru-db インストールスクリプト(Windows / Windows Server 共通)。
#
# 使い方(管理者権限のPowerShellで):
#   Invoke-WebRequest -Uri "https://github.com/aruaru-db/aruaru-db/releases/latest/download/aruaru-db-windows-x86_64.zip" -OutFile aruaru-db.zip
#   Expand-Archive aruaru-db.zip -DestinationPath aruaru-db
#   cd aruaru-db
#   .\install.ps1

#Requires -RunAsAdministrator

$ErrorActionPreference = "Stop"

$InstallDir = "C:\Program Files\aruaru-db"
$DataDir = "C:\ProgramData\aruaru-db"
$ServiceName = "AruaruDb"

Write-Host "==> インストール先: $InstallDir"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
New-Item -ItemType Directory -Force -Path $DataDir | Out-Null

$BinSrc = Join-Path $PSScriptRoot "aruaru-server.exe"
if (-not (Test-Path $BinSrc)) {
    Write-Error "aruaru-server.exe が見つかりません($BinSrc)。zipを展開したディレクトリで実行してください。"
    exit 1
}
Copy-Item $BinSrc -Destination $InstallDir -Force

$existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "==> 既存のWindowsサービスが見つかったため、バイナリのみ更新しました(再起動は行いません)"
    Write-Host "    手動で再起動する場合: Restart-Service $ServiceName"
} else {
    Write-Host "==> Windowsサービスとして登録する場合の手順:"
    Write-Host "      [Environment]::SetEnvironmentVariable('ARUARU_DATA_DIR', '$DataDir', 'Machine')"
    Write-Host "      New-Service -Name $ServiceName -BinaryPathName '$InstallDir\aruaru-server.exe' -DisplayName 'aruaru-db' -StartupType Automatic"
    Write-Host "      Start-Service $ServiceName"
}

Write-Host "==> 完了。"
