; Original PCBPlanner Studio shortcut metadata helper.
; Repair icon metadata without recreating shortcuts or changing their arguments/app identity.
!macro SetStudioShortcutIcon shortcut
  Push $0
  Push $1
  Push $2
  !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ShellLink} ${IID_IShellLink} r0 ""
  ${If} $0 P<> 0
    ${IUnknown::QueryInterface} $0 '("${IID_IPersistFile}",.r1)'
    ${If} $1 P<> 0
      ${IPersistFile::Load} $1 '("${shortcut}", ${STGM_READWRITE}).r2'
      ${If} $2 = 0
        ${IShellLink::SetIconLocation} $0 '("$INSTDIR\${MAINBINARYNAME}.exe", 0).r2'
        ${If} $2 = 0
          ${IPersistFile::Save} $1 '("${shortcut}",1).r2'
        ${EndIf}
      ${EndIf}
      ${IUnknown::Release} $1 ""
    ${EndIf}
    ${IUnknown::Release} $0 ""
  ${EndIf}
  Pop $2
  Pop $1
  Pop $0
!macroend
