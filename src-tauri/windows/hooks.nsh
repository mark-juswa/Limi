; On uninstall (not on update), remove Headroom's status line from Claude Code's
; settings and put back the user's original one.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --disconnect-claude'
  ${EndIf}
!macroend
