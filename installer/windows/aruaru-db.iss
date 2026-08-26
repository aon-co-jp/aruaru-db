; aruaru-db Windowsインストーラー(Inno Setup)。
;
; ユーザー指示「aruaru-llm.ps1などのパワーシェルでインストールする関連
; リポジトリは全て、リポジトリ名-installer.exe…に統一して」への対応。
;
; 正直な開示: aruaru-dbはWindowsサービス(`New-Service`)として登録
; される設計(既存`install.ps1`参照)——aruaru-llm/open-englishのような
; 単純なユーザー空間の実行ファイルとは異なり、サービス登録自体が
; 管理者権限を必要とする。そのため本インストーラーは
; `PrivilegesRequired=admin`とし、Inno Setup標準のUAC昇格プロンプトを
; 1回表示する(利用者が自分で管理者権限のPowerShellを探して起動する
; 手間を無くすことが目的——「管理者権限が一切不要になる」わけでは
; ない、これはサービス登録という機能の性質上避けられない)。
;
; ビルド方法: リポジトリルートで`cargo build --release --bin
; aruaru-server`を実行した後、このディレクトリで
; `ISCC.exe aruaru-db.iss`を実行する。

#define MyAppName "aruaru-db"
#ifndef MyAppVersion
  #define MyAppVersion "0.0.0-local-build"
#endif
#define MyAppPublisher "aon-co-jp"
#define MyAppURL "https://github.com/aon-co-jp/aruaru-db"
#define MyAppExeName "aruaru-server.exe"

[Setup]
; サービス登録(New-Service)には管理者権限が必須のため、既存の
; open-english/aruaru-llmインストーラーとは異なりlowestにできない
; (正直な開示、上記コメント参照)。
PrivilegesRequired=admin
AppId={{6B4F2A91-3D7C-4E58-9A02-1F6D8C5B3E77}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2
SolidCompression=yes
OutputDir=dist
; エコシステム全体の命名規則: <リポジトリ名>-installer.exe
; (バージョン番号なし、常に同じファイル名)。
OutputBaseFilename=aruaru-db-installer
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes

[Languages]
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "..\..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\install.ps1"; DestDir: "{app}"; Flags: ignoreversion

[Run]
; 既存install.ps1のサービス登録ロジックをそのまま再利用する(重複実装
; を避ける)。`-ExecutionPolicy Bypass`はこのプロセス限りの一時的な
; ものでシステム全体のポリシーは変更しない(既存open-english/
; aruaru-llmインストーラーと同じパターン)。
Filename: "powershell.exe"; \
    Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\install.ps1"""; \
    StatusMsg: "Registering aruaru-db as a Windows service... / aruaru-dbをWindowsサービスとして登録中..."; \
    Flags: waituntilterminated

[UninstallRun]
; アンインストール時にサービスも解除する(既存install.ps1はこの操作を
; 提供していないため、Inno Setup側で直接実行する)。`Remove-Service`は
; PowerShell 6+限定のコマンドレットで、既定のWindows PowerShell 5.1には
; 存在しないため(このマシンで実際に確認済み)、`sc.exe delete`を使う。
Filename: "powershell.exe"; \
    Parameters: "-NoProfile -Command ""Stop-Service AruaruDb -ErrorAction SilentlyContinue; sc.exe delete AruaruDb"""; \
    Flags: runhidden waituntilterminated; RunOnceId: "RemoveAruaruDbService"

[UninstallDelete]
Type: filesandordirs; Name: "{app}"
