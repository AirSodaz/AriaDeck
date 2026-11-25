; Inno Setup script for AriaDeck (RELEASE-001)
; Build after: scripts/package-windows-portable.ps1 -SkipZip
; Requires Inno Setup 6.3+ (6.6+ recommended for dark mode; 7.0+ for native x64 setup).
;
; Install location: per-user {autopf}\AriaDeck (%LocalAppData%\Programs\AriaDeck, no admin).
; Uninstall removes program files, shortcuts, and AriaDeck's registry values;
; user data under %LOCALAPPDATA%\AriaDeck is retained by default unless the user
; explicitly confirms removal (or passes /DeleteData=1 when uninstalling silently).

#if Ver < EncodeVer(6, 3, 0)
  #error Inno Setup 6.3.0 or later is required.
#endif

#define MyAppName "AriaDeck"
; Fallback only — packaging scripts pass /DMyAppVersion from Cargo.toml.
#ifndef MyAppVersion
  #define MyAppVersion "0.2.2"
#endif
#define MyAppPublisher "AriaDeck contributors"
#define MyAppURL "https://github.com/AirSodaz/AriaDeck"
#define MyAppExeName "ariadeck-desktop.exe"
#define MyBridgeExeName "ariadeck-bridge.exe"

; Browser bridge (D-045). Pass /DBridgeExtensionId=<32-char id> to offer the
; opt-in task. Without it the task is omitted entirely: registering a native
; messaging host with no pinned extension would let any extension launch it,
; which is the trust boundary the bridge design depends on.
#ifndef BridgeExtensionId
  #define BridgeExtensionId ""
#endif

#ifndef AppArch
  #define AppArch "x64"
#endif

#ifndef SourceDir
  #define SourceDir "..\..\dist\AriaDeck_" + MyAppVersion + "_windows-" + AppArch + "-portable"
#endif

; Derive strictly 4-part numeric FileVersion from MyAppVersion (strips -pre/+build).
#define private VerCore MyAppVersion
#if Pos("-", VerCore) > 0
  #define private VerCore Copy(VerCore, 1, Pos("-", VerCore) - 1)
#endif
#if Pos("+", VerCore) > 0
  #define private VerCore Copy(VerCore, 1, Pos("+", VerCore) - 1)
#endif
#define private VerRest VerCore
#define private VerDot1 Pos(".", VerRest)
#if VerDot1 > 0
  #define private VerRest Copy(VerRest, VerDot1 + 1)
  #define private VerDot2 Pos(".", VerRest)
  #if VerDot2 > 0
    #define private VerRest Copy(VerRest, VerDot2 + 1)
    #define private VerDot3 Pos(".", VerRest)
    #if VerDot3 > 0
      #define MyAppNumericVersion VerCore
    #else
      #define MyAppNumericVersion VerCore + ".0"
    #endif
  #else
    #define MyAppNumericVersion VerCore + ".0.0"
  #endif
#else
  #define MyAppNumericVersion VerCore + ".0.0.0"
#endif

; Official Simplified Chinese translation is built into Inno Setup 7.0.2+.
#define HasChineseSimplified FileExists(AddBackslash(CompilerPath) + "Languages\ChineseSimplified.isl")

#if Ver >= EncodeVer(6, 6, 1)
  #define AppFileFlags "ignoreversion notimestamp"
#else
  #define AppFileFlags "ignoreversion"
#endif

[Setup]
AppId={{A7B3C5D1-8E2F-4A6B-9C0D-1E2F3A4B5C6D}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
VersionInfoVersion={#MyAppNumericVersion}
VersionInfoProductName={#MyAppName}
VersionInfoProductVersion={#MyAppVersion}
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription={#MyAppName} Setup
VersionInfoCopyright=Copyright (c) {#MyAppPublisher}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
#if Ver >= EncodeVer(7, 0, 0)
SetupArchitecture=x64
#endif
#if AppArch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
OutputDir=..\..\dist
OutputBaseFilename=AriaDeck_{#MyAppVersion}_{#AppArch}-setup
#ifdef SetupIconFile
  #if FileExists(SetupIconFile)
SetupIconFile={#SetupIconFile}
  #endif
#endif
Compression=lzma2/max
SolidCompression=yes
#if Ver >= EncodeVer(6, 6, 0)
WizardStyle=modern dynamic
#else
WizardStyle=modern
#endif
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}
LicenseFile={#SourceDir}\LICENSE
; Do not touch %LOCALAPPDATA%\AriaDeck application data on uninstall by default.
CloseApplications=yes
ChangesAssociations=yes
#if HasChineseSimplified
ShowLanguageDialog=auto
#endif
#ifdef SignedInstall
SignTool=ariadeck
SignedUninstaller=yes
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
#if HasChineseSimplified
Name: "chinesesimplified"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"
#endif

[CustomMessages]
english.WindowsIntegrationGroup=Windows integration:
english.AssociateFilesTask=Associate .torrent, .metalink, and .meta4 files with AriaDeck
english.MagnetHandlerTask=Handle magnet links with AriaDeck
english.BrowserBridgeTask=Accept downloads from the AriaDeck browser extension (Chrome and Edge)
english.RegisteringBrowserBridge=Registering the browser bridge...
english.TorrentFileDesc=BitTorrent file
english.MetalinkFileDesc=Metalink file
english.Meta4FileDesc=Metalink v4 file
english.DeleteDataPrompt=AriaDeck has been uninstalled.%n%nDo you also want to delete your application data (%LOCALAPPDATA%\AriaDeck), including settings, profiles, and downloaded aria2 cores?
#if HasChineseSimplified
chinesesimplified.WindowsIntegrationGroup=Windows 集成:
chinesesimplified.AssociateFilesTask=将 .torrent、.metalink 和 .meta4 文件关联到 AriaDeck
chinesesimplified.MagnetHandlerTask=使用 AriaDeck 处理 magnet 磁力链接
chinesesimplified.BrowserBridgeTask=接收来自 AriaDeck 浏览器扩展（Chrome 和 Edge）的下载
chinesesimplified.RegisteringBrowserBridge=正在注册浏览器桥接程序...
chinesesimplified.TorrentFileDesc=BitTorrent 种子文件
chinesesimplified.MetalinkFileDesc=Metalink 下载描述文件
chinesesimplified.Meta4FileDesc=Metalink v4 下载描述文件
chinesesimplified.DeleteDataPrompt=AriaDeck 已卸载。%n%n是否同时删除应用数据（%LOCALAPPDATA%\AriaDeck），包括设置、配置文件和已下载的 aria2 内核？
#endif

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "fileassociations"; Description: "{cm:AssociateFilesTask}"; GroupDescription: "{cm:WindowsIntegrationGroup}"; Flags: unchecked
Name: "protocolhandlers"; Description: "{cm:MagnetHandlerTask}"; GroupDescription: "{cm:WindowsIntegrationGroup}"; Flags: unchecked
#if Len(BridgeExtensionId) > 0
Name: "browserbridge"; Description: "{cm:BrowserBridgeTask}"; GroupDescription: "{cm:WindowsIntegrationGroup}"; Flags: unchecked
#endif

[Files]
; Installed builds must NOT ship ariadeck.portable — data goes to LocalAppData.
Source: "{#SourceDir}\{#MyAppExeName}"; DestDir: "{app}"; Flags: {#AppFileFlags}
; Always installed; inert until the user opts into registering it as a host.
Source: "{#SourceDir}\{#MyBridgeExeName}"; DestDir: "{app}"; Flags: {#AppFileFlags}
Source: "{#SourceDir}\LICENSE"; DestDir: "{app}"; Flags: {#AppFileFlags} skipifsourcedoesntexist
Source: "{#SourceDir}\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: {#AppFileFlags} skipifsourcedoesntexist

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Registry]
; Application metadata for Windows "Open with" dialog.
Root: HKA; Subkey: "Software\Classes\Applications\{#MyAppExeName}"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "{#MyAppName}"; Flags: uninsdeletekey; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\Applications\{#MyAppExeName}\SupportedTypes"; ValueType: string; ValueName: ".torrent"; ValueData: ""; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\Applications\{#MyAppExeName}\SupportedTypes"; ValueType: string; ValueName: ".metalink"; ValueData: ""; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\Applications\{#MyAppExeName}\SupportedTypes"; ValueType: string; ValueName: ".meta4"; ValueData: ""; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\Applications\{#MyAppExeName}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" --open-metadata ""%1"""; Tasks: fileassociations

; Extension keys are shared. Remove only AriaDeck's values during uninstall, and clean up empty subkeys.
Root: HKA; Subkey: "Software\Classes\.torrent"; ValueType: string; ValueName: ""; ValueData: "AriaDeck.Torrent"; Flags: uninsdeletevalue uninsdeletekeyifempty; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\.torrent\OpenWithProgids"; ValueType: none; ValueName: "AriaDeck.Torrent"; Flags: uninsdeletevalue uninsdeletekeyifempty; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\.metalink"; ValueType: string; ValueName: ""; ValueData: "AriaDeck.Metalink"; Flags: uninsdeletevalue uninsdeletekeyifempty; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\.metalink\OpenWithProgids"; ValueType: none; ValueName: "AriaDeck.Metalink"; Flags: uninsdeletevalue uninsdeletekeyifempty; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\.meta4"; ValueType: string; ValueName: ""; ValueData: "AriaDeck.Meta4"; Flags: uninsdeletevalue uninsdeletekeyifempty; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\.meta4\OpenWithProgids"; ValueType: none; ValueName: "AriaDeck.Meta4"; Flags: uninsdeletevalue uninsdeletekeyifempty; Tasks: fileassociations

Root: HKA; Subkey: "Software\Classes\AriaDeck.Torrent"; ValueType: string; ValueName: ""; ValueData: "{cm:TorrentFileDesc}"; Flags: uninsdeletekey; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\AriaDeck.Torrent\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\AriaDeck.Torrent\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" --open-metadata ""%1"""; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\AriaDeck.Metalink"; ValueType: string; ValueName: ""; ValueData: "{cm:MetalinkFileDesc}"; Flags: uninsdeletekey; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\AriaDeck.Metalink\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\AriaDeck.Metalink\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" --open-metadata ""%1"""; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\AriaDeck.Meta4"; ValueType: string; ValueName: ""; ValueData: "{cm:Meta4FileDesc}"; Flags: uninsdeletekey; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\AriaDeck.Meta4\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"; Tasks: fileassociations
Root: HKA; Subkey: "Software\Classes\AriaDeck.Meta4\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" --open-metadata ""%1"""; Tasks: fileassociations

; The magnet scheme key is shared. Remove only AriaDeck's values on uninstall, and clean up empty subkeys.
Root: HKA; Subkey: "Software\Classes\magnet"; ValueType: string; ValueName: ""; ValueData: "URL:Magnet Link"; Flags: uninsdeletevalue uninsdeletekeyifempty; Tasks: protocolhandlers
Root: HKA; Subkey: "Software\Classes\magnet"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty; Tasks: protocolhandlers
Root: HKA; Subkey: "Software\Classes\magnet\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"; Flags: uninsdeletevalue uninsdeletekeyifempty; Tasks: protocolhandlers
Root: HKA; Subkey: "Software\Classes\magnet\shell"; Flags: uninsdeletekeyifempty; Tasks: protocolhandlers
Root: HKA; Subkey: "Software\Classes\magnet\shell\open"; Flags: uninsdeletekeyifempty; Tasks: protocolhandlers
Root: HKA; Subkey: "Software\Classes\magnet\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" --open-magnet ""%1"""; Flags: uninsdeletevalue uninsdeletekeyifempty; Tasks: protocolhandlers

Root: HKA; Subkey: "Software\Classes\AriaDeck.Magnet"; ValueType: string; ValueName: ""; ValueData: "URL:Magnet Link"; Flags: uninsdeletekey; Tasks: protocolhandlers
Root: HKA; Subkey: "Software\Classes\AriaDeck.Magnet"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: protocolhandlers
Root: HKA; Subkey: "Software\Classes\AriaDeck.Magnet\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"; Tasks: protocolhandlers
Root: HKA; Subkey: "Software\Classes\AriaDeck.Magnet\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" --open-magnet ""%1"""; Tasks: protocolhandlers

; Windows 10/11 Default Apps capabilities registration.
Root: HKA; Subkey: "Software\{#MyAppName}\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "{#MyAppName}"; Flags: uninsdeletekey; Tasks: fileassociations protocolhandlers
Root: HKA; Subkey: "Software\{#MyAppName}\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "Native desktop client for aria2"; Tasks: fileassociations protocolhandlers
Root: HKA; Subkey: "Software\{#MyAppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".torrent"; ValueData: "AriaDeck.Torrent"; Tasks: fileassociations
Root: HKA; Subkey: "Software\{#MyAppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".metalink"; ValueData: "AriaDeck.Metalink"; Tasks: fileassociations
Root: HKA; Subkey: "Software\{#MyAppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".meta4"; ValueData: "AriaDeck.Meta4"; Tasks: fileassociations
Root: HKA; Subkey: "Software\{#MyAppName}\Capabilities\URLAssociations"; ValueType: string; ValueName: "magnet"; ValueData: "AriaDeck.Magnet"; Tasks: protocolhandlers
Root: HKA; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "{#MyAppName}"; ValueData: "Software\{#MyAppName}\Capabilities"; Flags: uninsdeletevalue; Tasks: fileassociations protocolhandlers

[Run]
#if Len(BridgeExtensionId) > 0
; The host writes its own manifest and registry keys so the recorded path is
; always the real install path, and so a user can re-register after moving a
; portable copy. Runs before the app launch entry below.
Filename: "{app}\{#MyBridgeExeName}"; Parameters: "--register --extension-id {#BridgeExtensionId}"; StatusMsg: "{cm:RegisteringBrowserBridge}"; Flags: runhidden; Tasks: browserbridge
#endif
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Not gated on the task or on BridgeExtensionId: the host may also have been
; registered by hand, and --unregister is a no-op when nothing is registered.
; Runs before files are removed, so the executable still exists.
Filename: "{app}\{#MyBridgeExeName}"; Parameters: "--unregister"; RunOnceId: "UnregisterBrowserBridge"; Flags: runhidden skipifdoesntexist

[UninstallDelete]
; Backstop in case --unregister could not run; the registry keys it also removes
; are cleaned up by the entry above.
Type: files; Name: "{app}\com.ariadeck.bridge.json"

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: String;
  DeleteData: Boolean;
begin
  if CurUninstallStep = usUninstall then
  begin
    DataDir := ExpandConstant('{localappdata}\AriaDeck');
    if DirExists(DataDir) then
    begin
      if UninstallSilent then
        DeleteData := (ExpandConstant('{param:DeleteData|0}') = '1')
      else
        DeleteData := (SuppressibleMsgBox(
          ExpandConstant('{cm:DeleteDataPrompt}'),
          mbConfirmation,
          MB_YESNO or MB_DEFBUTTON2,
          IDNO
        ) = IDYES);

      if DeleteData then
        DelTree(DataDir, True, True, True);
    end;
  end;
end;
