' aruaru-DB Admin ランチャー
' Edge (WebView2) で PWA として開く。なければ既定ブラウザで開く。
Dim shell, fso, dir, index
Set shell = CreateObject("WScript.Shell")
Set fso   = CreateObject("Scripting.FileSystemObject")
dir   = fso.GetParentFolderName(WScript.ScriptFullName)
index = dir & "\index.html"

' Edge で PWA アプリモード起動を試みる
On Error Resume Next
shell.Run "msedge.exe --app=""file:///" & index & """", 1, False
If Err.Number <> 0 Then
  ' Edge がなければ既定ブラウザで開く
  shell.Run "explorer.exe """ & index & """", 1, False
End If
