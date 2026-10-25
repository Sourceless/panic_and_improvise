; The Windows installer: puts the launcher where the player chooses (Program Files to begin with), makes the
; shortcuts they ask for, and registers an uninstaller. The game is not in it. The launcher downloads the latest
; release the first time it runs and keeps it up to date, so this does not have to be run again for each release.
;
; Built by flake.nix with  makensis -DVERSION=... -DLAUNCHER=path\to\launcher.exe [-DOUTFILE=where]

Unicode true
!include "MUI2.nsh"
!include "x64.nsh"

!ifndef VERSION
  !define VERSION "0.1.0-dev"
!endif
!ifndef LAUNCHER
  !error "Pass -DLAUNCHER=<the launcher program>"
!endif

!define NAME "Panic and Improvise"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\PanicAndImprovise"

Name "${NAME}"
!ifndef OUTFILE
  !define OUTFILE "panic-and-improvise-installer-windows-x86_64.exe"
!endif
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\${NAME}"
; Where it went the last time, so that installing over a copy keeps it where it was.
InstallDirRegKey HKLM "Software\PanicAndImprovise" "InstallLocation"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
BrandingText "${NAME} ${VERSION}"

VIProductVersion "0.1.0.0"
VIAddVersionKey "LegalCopyright" "Laurence Pakenham-Smith"
VIAddVersionKey "ProductName" "${NAME}"
VIAddVersionKey "FileDescription" "${NAME} installer"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"

!define MUI_ABORTWARNING
!define MUI_COMPONENTSPAGE_SMALLDESC
!define MUI_FINISHPAGE_RUN "$INSTDIR\launcher.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Play ${NAME} now (the first time, this downloads the game)"
!define MUI_DIRECTORYPAGE_TEXT_TOP "The launcher goes in this folder, and so does the game it downloads. Choose another folder if you want them somewhere else."

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ; 64-bit Windows only: that is all the game is built for.
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "${NAME} needs a 64-bit version of Windows."
    Abort
  ${EndIf}
  SetRegView 64
FunctionEnd

Function un.onInit
  SetRegView 64
FunctionEnd

Section "${NAME} launcher (required)" SecLauncher
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "/oname=launcher.exe" "${LAUNCHER}"

  ; The game is kept in `data`, which the launcher reads from this file. It is the one place the launcher has to
  ; write to after this, so it alone is open to every user: the launcher's own folder stays as it is, and a
  ; program that anyone could replace is not left in Program Files.
  FileOpen $0 "$INSTDIR\location.txt" w
  FileWrite $0 "data$\r$\n"
  FileClose $0
  CreateDirectory "$INSTDIR\data"
  ; Users (S-1-5-32-545): modify, inherited by what is made in it.
  nsExec::Exec 'icacls "$INSTDIR\data" /grant *S-1-5-32-545:(OI)(CI)M /T /Q'
  Pop $0

  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "Software\PanicAndImprovise" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayName" "${NAME}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "Publisher" "Laurence Pakenham-Smith"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\launcher.exe"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegStr HKLM "${UNINSTALL_KEY}" "URLInfoAbout" "https://github.com/Sourceless/panic_and_improvise"
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Start menu shortcut" SecStartMenu
  SetShellVarContext all
  SetOutPath "$INSTDIR"
  CreateShortcut "$SMPROGRAMS\${NAME}.lnk" "$INSTDIR\launcher.exe" "" "$INSTDIR\launcher.exe" 0
SectionEnd

Section "Desktop shortcut" SecDesktop
  SetShellVarContext all
  SetOutPath "$INSTDIR"
  CreateShortcut "$DESKTOP\${NAME}.lnk" "$INSTDIR\launcher.exe" "" "$INSTDIR\launcher.exe" 0
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecLauncher} "The launcher, which downloads the latest ${NAME} and keeps it up to date every time you play."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecStartMenu} "A ${NAME} entry in the Start menu."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "A ${NAME} shortcut on the desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  SetShellVarContext all
  Delete "$SMPROGRAMS\${NAME}.lnk"
  Delete "$DESKTOP\${NAME}.lnk"
  ; The game and its version, which the launcher put there.
  RMDir /r "$INSTDIR\data"
  Delete "$INSTDIR\launcher.exe"
  Delete "$INSTDIR\location.txt"
  Delete "$INSTDIR\uninstall.exe"
  ; Only if nothing else has been put in it.
  RMDir "$INSTDIR"
  DeleteRegKey HKLM "${UNINSTALL_KEY}"
  DeleteRegKey HKLM "Software\PanicAndImprovise"
SectionEnd
