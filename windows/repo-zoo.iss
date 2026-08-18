; repo-zoo Inno Setup installer.
;
; Requires Inno Setup (https://jrsoftware.org/isinfo.php) and a release build.
; Build the binary first, then compile the installer:
;   cargo build --release
;   iscc windows\repo-zoo.iss
; The output setup.exe lands in windows\Output\.

#define MyAppName "repo-zoo"
#define MyAppVersion "0.1.0"
#define MyAppExeName "repo-zoo.exe"
#define MyAppPublisher "repo-zoo"
#define MyAppURL "https://github.com/anomalyco/opencode"

[Setup]
AppId={{D5B9F0A4-2C3E-4B8A-9E51-7A6C2B4D8E00}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
DefaultDirName={autopf}\repo-zoo
DefaultGroupName=repo-zoo
SetupIconFile=..\packaging\repo-zoo.ico
DisableProgramGroupPage=yes
OutputDir=Output
OutputBaseFilename=repo-zoo-setup
Compression=lzma
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\repo-zoo"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\repo-zoo"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Remove the per-user config so an uninstall is clean.
Type: filesandordirs; Name: "{userappdata}\repo-zoo"