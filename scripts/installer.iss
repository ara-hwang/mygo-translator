; Inno Setup script for the HyMT offline translator.
; CI builds the setup EXE with: ISCC.exe /DAppVersion=x.y.z scripts\installer.iss
#ifndef AppVersion
#define AppVersion "0.0.0-dev"
#endif

#define AppName "HyMT"
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
SetupIconFile=..\crates\hymt-app\assets\hymt.ico
UninstallDisplayIcon={app}\hymt.exe
OutputDir=..\dist
OutputBaseFilename=hymt-{#AppVersion}-windows-x64-setup
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
Source: "..\target\release\hymt.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\scripts\setup-runtime.ps1"; DestDir: "{app}\scripts"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\hymt.exe"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\hymt.exe"; Tasks: desktopicon

[Registry]
; The app writes this value itself (HKCU Run, autostart feature); delete it on uninstall.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "HyMT"; Flags: uninsdeletevalue
