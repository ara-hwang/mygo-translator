; Inno Setup script for the Offline Translator (mygo) build.
; CI builds the setup EXE with: ISCC.exe /DAppVersion=x.y.z scripts\installer.iss
#ifndef AppVersion
#define AppVersion "0.0.0-dev"
#endif

#define AppName "Offline Translator"
#define RepoURL "https://github.com/ara-hwang/offline-translator"

[Setup]
AppId={{7C0D5E2A-9B46-4F8B-A1D3-2E6F5C8907A4}}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisherURL={#RepoURL}
AppSupportURL={#RepoURL}/issues
AppUpdatesURL={#RepoURL}/releases
; Paths in .iss files resolve relative to this script's folder (scripts\).
DefaultDirName={autopf}\{#AppName}
SetupIconFile=..\resources\icon.ico
UninstallDisplayIcon={app}\offline-translator.exe
OutputDir=..\dist
OutputBaseFilename=offline-translator-{#AppVersion}-windows-x64-setup
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
DisableProgramGroupPage=yes
CloseApplications=yes
RestartApplications=no
ArchitecturesInstallIn64BitMode=x64compatible
ArchitecturesAllowed=x64compatible

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional icons:"

[Files]
Source: "..\dist\offline-translator.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\scripts\setup-runtime.ps1"; DestDir: "{app}\scripts"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\offline-translator.exe"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\offline-translator.exe"; Tasks: desktopicon

[Registry]
; The app itself writes this value (mygo SetOpenAtLogin, the Run key) under
; the app identifier from mygo.json; delete it on uninstall.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "app.offlinetranslator"; Flags: uninsdeletevalue
