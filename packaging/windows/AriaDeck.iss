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

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; Installed builds must NOT ship ariadeck.portable — data goes to LocalAppData.
Source: "{#SourceDir}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#SourceDir}\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

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
