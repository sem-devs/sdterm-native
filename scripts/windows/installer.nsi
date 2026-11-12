; The Windows installer of sdterm native, built with NSIS (see scripts/package-windows.ps1).
;
;   makensis /DROOT=<repo folder> /DVERSION=0.1.0 /DOUTFILE=<setup.exe> scripts\windows\installer.nsi
;
; It installs for the current user only (no administrator prompt) into
; %LOCALAPPDATA%\Programs\sdterm native, adds a Start menu shortcut and an entry in
; "Apps and features", and removes all of it again when uninstalled. Your data
; (%APPDATA%) is never touched.
;
;   /S        silent install
;   /UPDATE   used by the app itself: waits for the running app to quit, installs over it and
;             starts the new version again. Use it together with /S.

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user
ManifestDPIAware true

!ifndef ROOT
  !error "pass /DROOT=<repo folder>"
!endif
!ifndef VERSION
  !error "pass /DVERSION=<x.y.z>"
!endif
!ifndef OUTFILE
  !define OUTFILE "sdterm-native-${VERSION}-win-x64-setup.exe"
!endif

!define APP_NAME "sdterm native"
!define APP_EXE "sdterm-native.exe"
!define PUBLISHER "SEM Devs"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"

Name "${APP_NAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
InstallDirRegKey HKCU "Software\${APP_NAME}" "InstallDir"
BrandingText "${PUBLISHER}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "FileDescription" "${APP_NAME} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "SEM Devs, MIT license"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!insertmacro GetParameters
!insertmacro GetOptions
!insertmacro GetSize

!define MUI_ICON "${ROOT}\resources\icon.ico"
!define MUI_UNICON "${ROOT}\resources\icon.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP_NAME}"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Var UpdateMode

Function .onInit
  StrCpy $UpdateMode "0"
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/UPDATE" $R1
  ${IfNot} ${Errors}
    StrCpy $UpdateMode "1"
  ${EndIf}
FunctionEnd

; A program that is running cannot be overwritten, but it can be renamed. The old copy is deleted
; the next time. If even the rename fails, the app is probably still quitting: try again for a while.
Function MoveRunningAppAside
  Delete "$INSTDIR\${APP_EXE}.old"
  StrCpy $R2 "0"
  ${DoWhile} ${FileExists} "$INSTDIR\${APP_EXE}"
    ClearErrors
    Rename "$INSTDIR\${APP_EXE}" "$INSTDIR\${APP_EXE}.old"
    ${IfNot} ${Errors}
      ${Break}
    ${EndIf}
    IntOp $R2 $R2 + 1
    ${If} $R2 > 40
      ${Break}
    ${EndIf}
    Sleep 500
  ${Loop}
FunctionEnd

Section "Install"
  SetOutPath "$INSTDIR"
  Call MoveRunningAppAside
  File "${ROOT}\target\release\${APP_EXE}"
  File "${ROOT}\LICENSE"
  File "${ROOT}\README.md"

  ; Files the shells get handed (the app looks for a "resources" folder next to the program).
  RMDir /r "$INSTDIR\resources"
  SetOutPath "$INSTDIR\resources\shell"
  File /r "${ROOT}\resources\shell\*"
  SetOutPath "$INSTDIR"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortCut "$SMPROGRAMS\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0

  WriteRegStr HKCU "Software\${APP_NAME}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "https://sem-devs.com/sdterm"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" "$0"

  ; Started by the app to update itself: open the new version again.
  ${If} $UpdateMode == "1"
    Exec '"$INSTDIR\${APP_EXE}"'
  ${EndIf}
SectionEnd

Section "Uninstall"
  ; Only what the installer put there, never the whole folder: it may be one the user chose.
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\${APP_EXE}.old"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\README.md"
  RMDir /r "$INSTDIR\resources"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP_NAME}.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "Software\${APP_NAME}"
SectionEnd
