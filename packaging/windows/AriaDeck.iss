; Inno Setup script for AriaDeck (RELEASE-001)
; Build after: scripts/package-windows-portable.ps1 -SkipZip
; Requires Inno Setup 6+ (ISCC.exe on PATH or full path).
;
; Install location: per-user %LocalAppData%\Programs\AriaDeck (no admin).
; Uninstall removes program files and shortcuts only; user data under
; %LOCALAPPDATA%\AriaDeck is retained by default.

#define MyAppName "AriaDeck"
#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
#define MyAppPublisher "AriaDeck contributors"
#define MyAppExeName "ariadeck-desktop.exe"

#ifndef SourceDir
  #define SourceDir "..\..\dist\AriaDeck-" + MyAppVersion + "-windows-x64-portable"
#endif

[Setup]
AppId={{A7B3C5D1-8E2F-4A6B-9C0D-1E2F3A4B5C6D}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\dist
OutputBaseFilename=AriaDeck-{#MyAppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\{#MyAppExeName}
LicenseFile={#SourceDir}\LICENSE
InfoAfterFile=
; Do not touch %LOCALAPPDATA%\AriaDeck application data on uninstall.
CloseApplications=yes
ChangesAssociations=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "fileassociations"; Description: "Associate .torrent, .metalink, and .meta4 files with AriaDeck"; GroupDescription: "Windows integration:"; Flags: unchecked
Name: "protocolhandlers"; Description: "Handle magnet links with AriaDeck"; GroupDescription: "Windows integration:"; Flags: unchecked

[Files]
; Installed builds must NOT ship ariadeck.portable — data goes to LocalAppData.
Source: "{#SourceDir}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#SourceDir}\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Registry]
; Extension keys are shared. Remove only AriaDeck's values during uninstall.
Root: HKCU; Subkey: "Software\Classes\.torrent"; ValueType: string; ValueName: ""; ValueData: "AriaDeck.Torrent"; Flags: uninsdeletevalue; Tasks: fileassociations
Root: HKCU; Subkey: "Software\Classes\.torrent\OpenWithProgids"; ValueType: none; ValueName: "AriaDeck.Torrent"; Flags: uninsdeletevalue; Tasks: fileassociations
Root: HKCU; Subkey: "Software\Classes\.metalink"; ValueType: string; ValueName: ""; ValueData: "AriaDeck.Metalink"; Flags: uninsdeletevalue; Tasks: fileassociations
Root: HKCU; Subkey: "Software\Classes\.metalink\OpenWithProgids"; ValueType: none; ValueName: "AriaDeck.Metalink"; Flags: uninsdeletevalue; Tasks: fileassociations
Root: HKCU; Subkey: "Software\Classes\.meta4"; ValueType: string; ValueName: ""; ValueData: "AriaDeck.Meta4"; Flags: uninsdeletevalue; Tasks: fileassociations
Root: HKCU; Subkey: "Software\Classes\.meta4\OpenWithProgids"; ValueType: none; ValueName: "AriaDeck.Meta4"; Flags: uninsdeletevalue; Tasks: fileassociations

Root: HKCU; Subkey: "Software\Classes\AriaDeck.Torrent"; ValueType: string; ValueName: ""; ValueData: "BitTorrent file"; Flags: uninsdeletekey; Tasks: fileassociations
Root: HKCU; Subkey: "Software\Classes\AriaDeck.Torrent\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"; Tasks: fileassociations
Root: HKCU; Subkey: "Software\Classes\AriaDeck.Torrent\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" --open-metadata ""%1"""; Tasks: fileassociations
Root: HKCU; Subkey: "Software\Classes\AriaDeck.Metalink"; ValueType: string; ValueName: ""; ValueData: "Metalink file"; Flags: uninsdeletekey; Tasks: fileassociations
Root: HKCU; Subkey: "Software\Classes\AriaDeck.Metalink\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"; Tasks: fileassociations
Root: HKCU; Subkey: "Software\Classes\AriaDeck.Metalink\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" --open-metadata ""%1"""; Tasks: fileassociations
Root: HKCU; Subkey: "Software\Classes\AriaDeck.Meta4"; ValueType: string; ValueName: ""; ValueData: "Metalink v4 file"; Flags: uninsdeletekey; Tasks: fileassociations
Root: HKCU; Subkey: "Software\Classes\AriaDeck.Meta4\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"; Tasks: fileassociations
Root: HKCU; Subkey: "Software\Classes\AriaDeck.Meta4\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" --open-metadata ""%1"""; Tasks: fileassociations

; The magnet scheme key is shared. Remove only AriaDeck's values on uninstall.
Root: HKCU; Subkey: "Software\Classes\magnet"; ValueType: string; ValueName: ""; ValueData: "URL:Magnet Link"; Flags: uninsdeletevalue; Tasks: protocolhandlers
Root: HKCU; Subkey: "Software\Classes\magnet"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Flags: uninsdeletevalue; Tasks: protocolhandlers
Root: HKCU; Subkey: "Software\Classes\magnet\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"; Flags: uninsdeletevalue; Tasks: protocolhandlers
Root: HKCU; Subkey: "Software\Classes\magnet\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" --open-magnet ""%1"""; Flags: uninsdeletevalue; Tasks: protocolhandlers

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[Code]
// Optional: offer data removal only when the user explicitly checks the box.
var
  DeleteDataCheck: TNewCheckBox;

procedure InitializeUninstallProgressForm;
begin
  DeleteDataCheck := TNewCheckBox.Create(UninstallProgressForm);
  DeleteDataCheck.Parent := UninstallProgressForm.InnerNotebook.Pages[0];
  DeleteDataCheck.Left := UninstallProgressForm.StatusLabel.Left;
  DeleteDataCheck.Top := UninstallProgressForm.StatusLabel.Top + 40;
  DeleteDataCheck.Width := UninstallProgressForm.StatusLabel.Width;
  DeleteDataCheck.Caption := 'Also delete application data (%LOCALAPPDATA%\AriaDeck)';
  DeleteDataCheck.Checked := False;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    if (DeleteDataCheck <> nil) and DeleteDataCheck.Checked then
    begin
      DataDir := ExpandConstant('{localappdata}\AriaDeck');
      if DirExists(DataDir) then
        DelTree(DataDir, True, True, True);
    end;
  end;
end;
