; repo-zoo NSIS installer script.
;
; Build the Windows binary first, then compile the installer from Linux (or
; anywhere makensis runs):
;   make win-setup
;   # or manually:
;   makensis windows/repo-zoo.nsi
; The setup.exe lands in windows/Output/.
;
; For cross-compiled layouts pass the exe explicitly, e.g.:
;   makensis -DEXE=../target/x86_64-pc-windows-gnu/release/repo-zoo.exe windows/repo-zoo.nsi
;
; This is the Linux-buildable equivalent of windows/repo-zoo.iss (Inno Setup).

!include "MUI2.nsh"

!ifndef EXE
  !define EXE "../target/release/repo-zoo.exe"
!endif
!ifndef VERSION
  !define VERSION "0.1.0"
!endif

!define APPNAME "repo-zoo"

Name "${APPNAME} ${VERSION}"
OutFile "Output/repo-zoo-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\repo-zoo"
RequestExecutionLevel user
SetCompressor /SOLID lzma

; Version info embedded into the setup.exe.
VIProductVersion "0.1.0.0"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "FileDescription" "repo-zoo code project launcher"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "CompanyName" "repo-zoo"
VIAddVersionKey "LegalCopyright" "© repo-zoo"

!define MUI_ICON "../packaging/repo-zoo.ico"
!define MUI_UNICON "../packaging/repo-zoo.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\repo-zoo.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Run repo-zoo"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "repo-zoo (required)" SecMain
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "${EXE}"

  CreateDirectory "$SMPROGRAMS\repo-zoo"
  CreateShortcut "$SMPROGRAMS\repo-zoo\repo-zoo.lnk" "$INSTDIR\repo-zoo.exe"

  WriteUninstaller "$INSTDIR\Uninstall repo-zoo.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\repo-zoo" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\repo-zoo" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\repo-zoo" "Publisher" "repo-zoo"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\repo-zoo" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\repo-zoo" "DisplayIcon" "$INSTDIR\repo-zoo.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\repo-zoo" "UninstallString" '"$INSTDIR\Uninstall repo-zoo.exe"'
SectionEnd

Section "Desktop shortcut" SecDesktop
  CreateShortcut "$DESKTOP\repo-zoo.lnk" "$INSTDIR\repo-zoo.exe"
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\repo-zoo.exe"
  Delete "$INSTDIR\Uninstall repo-zoo.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\repo-zoo\repo-zoo.lnk"
  RMDir "$SMPROGRAMS\repo-zoo"
  Delete "$DESKTOP\repo-zoo.lnk"

  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\repo-zoo"

  ; Remove the per-user config so an uninstall is clean.
  RMDir /r "$APPDATA\repo-zoo"
SectionEnd