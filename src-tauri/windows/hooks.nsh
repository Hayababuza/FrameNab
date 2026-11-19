; Installer hooks for the NSIS setup (see `bundle.windows.nsis.installerHooks` in tauri.conf.json).

; FrameNapper used to be called FrameNab, which Windows treats as a different app: without this,
; both stayed installed side by side. Quit and silently uninstall FrameNab first. Its settings and
; recordings stay on disk (a silent uninstall keeps the app data); FrameNapper takes them over on
; its first start (src-tauri/src/rename.rs).
!macro NSIS_HOOK_PREINSTALL
  ReadRegStr $R8 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\FrameNab" "InstallLocation"
  ${If} $R8 != ""
    ; Stored with quotes: "C:\Users\...\FrameNab"
    StrCpy $R9 $R8 1
    ${If} $R9 == '"'
      StrCpy $R8 $R8 -1 1
    ${EndIf}
    ${If} ${FileExists} "$R8\uninstall.exe"
      DetailPrint "Removing FrameNab (the previous name of FrameNapper)"
      nsExec::Exec 'taskkill /F /IM framenab.exe'
      Pop $R9
      ; `_?=` runs the uninstaller in place, so ExecWait really waits for it.
      ExecWait '"$R8\uninstall.exe" /S _?=$R8'
      Delete "$R8\uninstall.exe"
      RMDir "$R8"
    ${EndIf}
  ${EndIf}
!macroend
