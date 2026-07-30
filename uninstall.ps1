# aruaru-db アンインストールスクリプト(Windows / Windows Server 共通、
# install.ps1と対になる新規スクリプト、2026-07-30追記)。
#
# **安全性の設計方針(最重要)**: `install.ps1`が作成する
# `C:\ProgramData\aruaru-db`(ARUARU_DATA_DIR)には実際のDBデータが
# 入っている。このスクリプトは**データディレクトリを絶対に削除しない**
# ——バイナリとWindowsサービス登録のみを削除し、データディレクトリの
# 存在と場所を明示して終了する。
#
# 使い方(管理者権限のPowerShellで):
#   cd "C:\Program Files\aruaru-db"
#   .\uninstall.ps1

#Requires -RunAsAdministrator

$ErrorActionPreference = "Stop"

$InstallDir = "C:\Program Files\aruaru-db"
$DataDir = "C:\ProgramData\aruaru-db"
$ServiceName = "AruaruDb"
$BinPath = Join-Path $InstallDir "aruaru-server.exe"

$existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($existing) {
    if ($existing.Status -eq "Running") {
        Write-Host "==> $ServiceName サービスを停止"
        Stop-Service -Name $ServiceName -Force
    }
    Write-Host "==> $ServiceName サービス登録を削除"
    sc.exe delete $ServiceName | Out-Null
}

if (Test-Path $BinPath) {
    Write-Host "==> $BinPath を削除"
    Remove-Item -Path $BinPath -Force
} else {
    Write-Host "==> $BinPath は見つかりませんでした(既に削除済み)"
}

Write-Host "==> 完了。"
if (Test-Path $DataDir) {
    Write-Host "    データディレクトリ $DataDir は意図的に削除していません。"
    Write-Host "    別バージョンの再インストール時にもこのデータはそのまま利用されます。"
    Write-Host "    データも含めて完全に削除したい場合のみ、内容を確認の上で手動で"
    Write-Host ("    Remove-Item -Recurse -Force '{0}' を実行してください(このスクリプトは自動実行しません)。" -f $DataDir)
} else {
    Write-Host "    データディレクトリ $DataDir は見つかりませんでした。"
}
