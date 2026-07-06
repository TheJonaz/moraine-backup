; Inno Setup script for the Moraine Windows desktop app (GTK GUI).
;
; Build (in .github/workflows/windows-gui.yml):
;   iscc /DMyAppVersion=0.1.24 /DSrcDir=<bundle dir> packaging\windows\moraine-gui.iss
;
; Installs the bundled GTK runtime + moraine-gui.exe per-user (no admin) and
; creates a Start menu (and optional Desktop) shortcut that launches the app.

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif
#ifndef SrcDir
  #define SrcDir "..\..\bundle"
#endif
#define MyAppName "Moraine"
#define MyAppPublisher "Jonaz Thern"
#define MyAppURL "https://moraine.thern.io"
#define MyAppExeName "moraine-gui.exe"

[Setup]
AppId={{2F8A5D31-9C4E-4B7A-8E1F-6A3D9C2B4E5F}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=Output
OutputBaseFilename=moraine-gui-{#MyAppVersion}-setup
SetupIconFile=..\..\assets\moraine.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64

[Tasks]
Name: "desktopicon"; Description: "Create a Desktop shortcut"; GroupDescription: "Shortcuts:"

[Files]
Source: "{#SrcDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Moraine"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\moraine.ico"
Name: "{autodesktop}\Moraine"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\moraine.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch Moraine"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent
