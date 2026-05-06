; Kill any running App Launcher process before installing.
; nsExec::Exec is fire-and-forget — non-zero exit (nothing running) is ignored.
nsExec::Exec 'taskkill /F /IM "App Launcher.exe"'
Pop $0
