; Desktop uninstall owns only the installed desktop files and shortcuts.
; The shared per-user daemon, native service, current release, access token,
; and database outlive the client, including /UPDATE and silent uninstall.
;
; Tauri's built-in Delete app data checkbox would recursively remove both
; $APPDATA\${BUNDLEID} and $LOCALAPPDATA\${BUNDLEID}. The latter contains the
; shared daemon. Clear its state before Tauri reaches that deletion branch.
; Daemon removal is a separate explicitly confirmed in-app / CLI operation.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
  ${AndIf} $DeleteAppDataCheckboxState = 1
    MessageBox MB_ICONINFORMATION|MB_OK "Desktop uninstall preserves the shared Amarcode daemon and its data. To remove the daemon on this computer, use Settings > General > Remove local daemon and data before uninstalling, or run amarcode-daemon purge --confirm-data-loss separately." /SD IDOK
  ${EndIf}
  StrCpy $DeleteAppDataCheckboxState 0
  DetailPrint "Preserving the shared Amarcode daemon, service, and data."
!macroend
