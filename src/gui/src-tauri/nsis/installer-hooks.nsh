!include "LogicLib.nsh"

!define LOADBOT_HOOK_DIR "${__FILEDIR__}"

!macro NSIS_HOOK_PREINSTALL
  StrCpy $INSTDIR "$LOCALAPPDATA\Programs\Loadbot"
  SetOutPath "$INSTDIR"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  File /oname=$PLUGINSDIR\loadbot-user-path.ps1 "${LOADBOT_HOOK_DIR}\user-path.ps1"
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\loadbot-user-path.ps1" -Mode Add -InstallDir "$INSTDIR"'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_ICONEXCLAMATION "Loadbot was installed, but its CLI could not be added to your user PATH. Add $INSTDIR manually."
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  File /oname=$PLUGINSDIR\loadbot-user-path.ps1 "${LOADBOT_HOOK_DIR}\user-path.ps1"
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\loadbot-user-path.ps1" -Mode Remove -InstallDir "$INSTDIR"'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_ICONEXCLAMATION "Loadbot was removed, but its user PATH entry could not be cleaned up."
  ${EndIf}
!macroend
