; Inno Setup script for the Moraine Windows CLI.
;
; Build (done automatically by .github/workflows/release.yml):
;   iscc /DMyAppVersion=0.1.25 /DSrcDir=<staged files> packaging\windows\moraine.iss
;
; Produces Output\moraine-<ver>-setup.exe — a per-user installer (no admin / no
; UAC) that installs moraine.exe and adds it to the user's PATH, so `moraine`
; runs from any new terminal. Uninstall via Settings → Apps.

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif
#ifndef SrcDir
  #define SrcDir "..\..\dist\moraine-windows-x86_64"
#endif
#define MyAppName "Moraine"
#define MyAppPublisher "Jonaz Thern"
#define MyAppURL "https://moraine.thern.io"
#define MyAppExeName "moraine.exe"

[Setup]
AppId={{7E9B2C1A-4F3D-4A6B-9C2E-1D8F5A0B3C7D}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=Output
OutputBaseFilename=moraine-{#MyAppVersion}-setup
SetupIconFile=..\..\assets\moraine.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64
ChangesEnvironment=yes

[Tasks]
Name: "addtopath"; Description: "Add Moraine to my PATH (run ""moraine"" from any terminal)"; GroupDescription: "Options:"
Name: "startmenuicon"; Description: "Add a Start menu shortcut"; GroupDescription: "Shortcuts:"
Name: "desktopicon"; Description: "Add a Desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "{#SrcDir}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SrcDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#SrcDir}\CHANGELOG.md"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#SrcDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#SrcDir}\moraine.example.toml"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
; Moraine is a CLI, so the shortcuts open a terminal in the install folder with
; `moraine` ready to use (and carry the Moraine icon). Optional per the tasks above.
Name: "{autoprograms}\Moraine"; Filename: "{cmd}"; Parameters: "/K moraine --help"; WorkingDir: "{app}"; IconFilename: "{app}\{#MyAppExeName}"; Comment: "Open a terminal with the Moraine CLI"; Tasks: startmenuicon
Name: "{autodesktop}\Moraine"; Filename: "{cmd}"; Parameters: "/K moraine --help"; WorkingDir: "{app}"; IconFilename: "{app}\{#MyAppExeName}"; Comment: "Open a terminal with the Moraine CLI"; Tasks: desktopicon

[Registry]
; Append {app} to the per-user PATH (only if the task is chosen and it isn't
; already there). ChangesEnvironment broadcasts the change so new terminals see it.
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Tasks: addtopath; Check: NeedsAddPath('{app}')

[Code]
function NeedsAddPath(Param: string): Boolean;
var
  OrigPath: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', OrigPath) then
  begin
    Result := True;
    exit;
  end;
  { True (add it) only when the folder isn't already on PATH. }
  Result := Pos(';' + Uppercase(ExpandConstant(Param)) + ';', ';' + Uppercase(OrigPath) + ';') = 0;
end;
