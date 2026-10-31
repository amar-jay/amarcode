; Isolated harness for the production pre-uninstall hook. It models Tauri's
; application-file removal and subsequent recursive Delete app data branch.
; No native task, registry, or real application-data directory is touched.
Unicode true
!include LogicLib.nsh
!include FileFunc.nsh
!ifndef FIXTURE_OUT
  !error "Set FIXTURE_OUT to a temporary installer path"
!endif
!ifndef HOOK_FILE
  !error "Set HOOK_FILE to the production installer-hooks.nsh path"
!endif
!include "${HOOK_FILE}"
Var UpdateMode
Var DeleteAppDataCheckboxState
OutFile "${FIXTURE_OUT}"
RequestExecutionLevel user
SilentInstall silent
SilentUnInstall silent

Section
  CreateDirectory "$INSTDIR"
  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

Function un.onInit
  StrCpy $UpdateMode 0
  StrCpy $DeleteAppDataCheckboxState 0
  ${GetParameters} $0
  ${GetOptions} $0 "/DELETE_APP_DATA=" $DeleteAppDataCheckboxState
  ClearErrors
  ${GetOptions} $0 "/UPDATE" $1
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
FunctionEnd

Section Uninstall
  !insertmacro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\amarcode.exe"
  Delete "$INSTDIR\amarcode-bootstrap.exe"

  ; Tauri checks this state after NSIS_HOOK_PREUNINSTALL, then removes both
  ; bundle-ID data roots. These fixture folders represent those directories.
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    RMDir /r "$INSTDIR\..\local-app-data"
    RMDir /r "$INSTDIR\..\roaming-app-data"
  ${EndIf}

  FileOpen $0 "$INSTDIR\hook-result.txt" w
  FileWrite $0 "$DeleteAppDataCheckboxState"
  FileClose $0
SectionEnd
