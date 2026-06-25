; aruaru-DB Admin — NSIS インストーラ
; ビルド: makensis setup.nsi
; 前提: aruaru-db-admin-web の dist/ を ../web-dist/ にコピーしてから実行

Unicode True
!define APP_NAME      "aruaru-DB Admin"
!define APP_VERSION   "0.5.0"
!define APP_PUBLISHER "aruaru Project"
!define INSTALL_DIR   "$PROGRAMFILES64\aruaru-DB Admin"
!define UNINSTALLER   "Uninstall.exe"
!define WEBVIEW2_URL  "https://go.microsoft.com/fwlink/p/?LinkId=2124703"

Name "${APP_NAME} ${APP_VERSION}"
OutFile "aruaru-db-admin-setup-${APP_VERSION}.exe"
InstallDir "${INSTALL_DIR}"
RequestExecutionLevel admin
SetCompressor /SOLID lzma

!include "MUI2.nsh"
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Japanese"

Section "メインアプリ" SecMain
  SetOutPath "$INSTDIR"

  ; Web アセット (PWA dist) をコピー
  File /r "..\web-dist\*.*"

  ; ランチャー (WebView2 で index.html を開く)
  File "launcher.cmd"
  File "aruaru-admin.vbs"   ; コンソールウィンドウなしで起動

  ; アンインストーラを生成
  WriteUninstaller "$INSTDIR\${UNINSTALLER}"

  ; スタートメニュー
  CreateDirectory "$SMPROGRAMS\aruaru-DB"
  CreateShortcut "$SMPROGRAMS\aruaru-DB\aruaru-DB Admin.lnk" \
    "$INSTDIR\aruaru-admin.vbs" "" "$INSTDIR\icon.ico"
  CreateShortcut "$SMPROGRAMS\aruaru-DB\アンインストール.lnk" \
    "$INSTDIR\${UNINSTALLER}"

  ; デスクトップショートカット
  CreateShortcut "$DESKTOP\aruaru-DB Admin.lnk" \
    "$INSTDIR\aruaru-admin.vbs" "" "$INSTDIR\icon.ico"

  ; レジストリ (プログラムの追加と削除)
  WriteRegStr   HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\aruaru-DB-Admin" \
    "DisplayName" "${APP_NAME}"
  WriteRegStr   HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\aruaru-DB-Admin" \
    "DisplayVersion" "${APP_VERSION}"
  WriteRegStr   HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\aruaru-DB-Admin" \
    "Publisher" "${APP_PUBLISHER}"
  WriteRegStr   HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\aruaru-DB-Admin" \
    "UninstallString" "$INSTDIR\${UNINSTALLER}"
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\${UNINSTALLER}"
  RMDir /r "$INSTDIR"
  Delete "$DESKTOP\aruaru-DB Admin.lnk"
  RMDir /r "$SMPROGRAMS\aruaru-DB"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\aruaru-DB-Admin"
SectionEnd
