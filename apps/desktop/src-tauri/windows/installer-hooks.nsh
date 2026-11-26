; Hooks into the NSIS installer Tauri builds (bundle.windows.nsis.installerHooks).
; The app itself puts its bin folder (bin\webide.cmd) on the user's PATH, and
; takes it off again: the same code as its "Install 'webide' Command in PATH".

!macro NSIS_HOOK_POSTINSTALL
  nsExec::Exec '"$INSTDIR\webide.exe" --install-command-line'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec '"$INSTDIR\webide.exe" --uninstall-command-line'
!macroend
