; Amarcode native-uninstaller cleanup.
;
; Tauri passes /UPDATE when the uninstaller is being used to replace an
; existing installation. Never touch the daemon service or user data in that
; mode. On a real uninstall the service registration must always go away;
; daemon data is removed only when the user selects Tauri's "Delete app data"
; checkbox.
${Using:StrFunc} UnStrLoc

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    DetailPrint "Checking Amarcode background service..."
    nsExec::ExecToStack '"$SYSDIR\schtasks.exe" /Query /TN "Amarcode Daemon"'
    Pop $0
    Pop $1

    ; A missing task is already the desired state. If it exists, stop it and
    ; require successful unregistration before continuing the uninstall.
    ${If} $0 = 0
      DetailPrint "Stopping Amarcode background service..."
      nsExec::ExecToLog '"$SYSDIR\schtasks.exe" /End /TN "Amarcode Daemon"'
      Pop $0

      DetailPrint "Removing Amarcode background service..."
      nsExec::ExecToStack '"$SYSDIR\schtasks.exe" /Delete /TN "Amarcode Daemon" /F'
      Pop $0
      Pop $1
      ${If} $0 <> 0
        MessageBox MB_ICONSTOP|MB_OK "Amarcode could not remove its background service. Close running Amarcode processes and retry the uninstall.$\r$\n$\r$\n$1"
        SetErrorLevel 1
        Abort
      ${EndIf}
    ${EndIf}

    ; Task deletion does not itself guarantee that the launched executable has
    ; exited. Refuse data deletion while any daemon process remains.
    Sleep 500
    nsExec::ExecToStack '"$SYSDIR\tasklist.exe" /FI "IMAGENAME eq amarcode-daemon.exe" /FO CSV /NH'
    Pop $0
    Pop $1
    ${If} $0 <> 0
      MessageBox MB_ICONSTOP|MB_OK "Amarcode could not verify that its background service stopped. Retry the uninstall."
      SetErrorLevel 1
      Abort
    ${EndIf}
    ${UnStrLoc} $2 $1 '"amarcode-daemon.exe",' ">"
    ${If} $2 = 0
      MessageBox MB_ICONSTOP|MB_OK "The Amarcode background service is still running. Close it and retry the uninstall."
      SetErrorLevel 1
      Abort
    ${EndIf}

    ${If} $DeleteAppDataCheckboxState = 1
      DetailPrint "Removing Amarcode daemon data..."
      ${If} ${FileExists} "$LOCALAPPDATA\amarcode\*.*"
        ; Never recurse through a junction or other reparse point.
        System::Call 'kernel32::GetFileAttributesW(w "$LOCALAPPDATA\amarcode") i .r0'
        IntOp $1 $0 & 0x400
        ${If} $1 <> 0
          MessageBox MB_ICONSTOP|MB_OK "Amarcode data is stored at a redirected filesystem location. The uninstaller will not delete it automatically: $LOCALAPPDATA\amarcode"
          SetErrorLevel 1
          Abort
        ${EndIf}
        RMDir /r "$LOCALAPPDATA\amarcode"
        ${If} ${FileExists} "$LOCALAPPDATA\amarcode\*.*"
          MessageBox MB_ICONSTOP|MB_OK "Amarcode could not remove its daemon data. Close running Amarcode processes and retry the uninstall."
          SetErrorLevel 1
          Abort
        ${EndIf}
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend
