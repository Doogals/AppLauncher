!macro NSIS_HOOK_PREINSTALL
  ; Kill any running TakeOff process before installing.
  ; nsExec::Exec is fire-and-forget - non-zero exit (nothing running) is ignored.
  ; "App Launcher.exe" is the pre-rebrand binary name, kept for old installs.
  nsExec::Exec 'taskkill /F /IM "TakeOff.exe"'
  Pop $0
  nsExec::Exec 'taskkill /F /IM "App Launcher.exe"'
  Pop $0
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Remove the logon task and Run value so an uninstall leaves nothing that
  ; tries to start a missing exe at every sign-in. If this is an update, the
  ; new version re-registers on its first launch.
  nsExec::Exec 'schtasks /Delete /F /TN "TakeOffAutostart"'
  Pop $0
  nsExec::Exec 'schtasks /Delete /F /TN "TakeOff"'
  Pop $0
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "TakeOff"
!macroend
