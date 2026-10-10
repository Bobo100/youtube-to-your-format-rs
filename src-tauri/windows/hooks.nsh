; The app keeps its tools, logs and settings in %LOCALAPPDATA%\youtube-to-your-format,
; which is also the default install folder. Tauri's "delete app data" checkbox only
; removes %LOCALAPPDATA%\<identifier>, so clear our folder here. Only names the app
; writes are removed; anything else the user put there stays.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RMDir /r "$LOCALAPPDATA\youtube-to-your-format\bin"
    RMDir /r "$LOCALAPPDATA\youtube-to-your-format\logs"
    Delete "$LOCALAPPDATA\youtube-to-your-format\settings.json"
    Delete "$LOCALAPPDATA\youtube-to-your-format\settings.broken.json"
    Delete "$LOCALAPPDATA\youtube-to-your-format\settings.json.tmp"
    RMDir "$LOCALAPPDATA\youtube-to-your-format"
  ${EndIf}
!macroend
