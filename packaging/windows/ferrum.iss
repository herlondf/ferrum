; Inno Setup script for the Windows installer (ferrum-windows-x86_64-setup.exe).
; Built by scripts/package-windows.ps1:
;   iscc /DAppVersion=0.2.0 /DSourceDir=..\..\target\release /DOutputDir=..\..\dist packaging\windows\ferrum.iss
; Installs per user by default (no admin); the first page offers "for all users".
; The in-app updater runs it with /SILENT /CLOSEAPPLICATIONS /RELAUNCH=1.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\target\release"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

[Setup]
; Never change the AppId: Windows (and winget) use it to find the installed copy.
AppId={{6E1B0F5A-3C2D-4F7B-9A4E-F3E2A1C0B7D9}
AppName=Ferrum
AppVersion={#AppVersion}
AppVerName=Ferrum {#AppVersion}
AppPublisher=Herlon D. Filgueira
AppPublisherURL=https://github.com/herlondf/ferrum
AppSupportURL=https://github.com/herlondf/ferrum/issues
AppUpdatesURL=https://github.com/herlondf/ferrum/releases
DefaultDirName={autopf}\Ferrum
DefaultGroupName=Ferrum
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename=ferrum-windows-x86_64-setup
SetupIconFile=ferrum.ico
UninstallDisplayIcon={app}\ferrum.ico
LicenseFile=..\..\LICENSE-MIT
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
ChangesEnvironment=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "addtopath"; Description: "Add ferrum to the PATH"; GroupDescription: "Command line:"

[Files]
Source: "{#SourceDir}\ferrum.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "ferrum.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Ferrum"; Filename: "{app}\ferrum.exe"; IconFilename: "{app}\ferrum.ico"
Name: "{autodesktop}\Ferrum"; Filename: "{app}\ferrum.exe"; IconFilename: "{app}\ferrum.ico"; Tasks: desktopicon

[Registry]
; The user's PATH (or the machine's, when installed for all users).
Root: HKA; Subkey: "{code:EnvKey}"; ValueType: expandsz; ValueName: "Path"; \
  ValueData: "{olddata};{app}"; Tasks: addtopath; Check: NeedsAddPath(ExpandConstant('{app}'))

[Run]
Filename: "{app}\ferrum.exe"; Description: "{cm:LaunchProgram,Ferrum}"; Flags: nowait postinstall skipifsilent
; After an in-app update (silent), start the new version.
Filename: "{app}\ferrum.exe"; Flags: nowait; Check: Relaunch

[Code]
function EnvKey(Param: String): String;
begin
  if IsAdminInstallMode then
    Result := 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment'
  else
    Result := 'Environment';
end;

function NeedsAddPath(Dir: String): Boolean;
var
  Path: String;
  Root: Integer;
begin
  if IsAdminInstallMode then Root := HKEY_LOCAL_MACHINE else Root := HKEY_CURRENT_USER;
  if not RegQueryStringValue(Root, EnvKey(''), 'Path', Path) then
  begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Path) + ';') = 0;
end;

function Relaunch: Boolean;
begin
  Result := WizardSilent and (ExpandConstant('{param:RELAUNCH|0}') = '1');
end;
