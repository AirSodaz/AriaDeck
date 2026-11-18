#Requires -Version 5.1
<#
.SYNOPSIS
  Build a Windows portable package for AriaDeck (RELEASE-001).

.DESCRIPTION
  Stages dist/AriaDeck_<version>_windows-<arch>-portable/ with:
    - ariadeck-desktop.exe
    - ariadeck-bridge.exe  (browser bridge native messaging host, D-045)
    - ariadeck.portable  (enables <exe_dir>/data)
    - LICENSE, THIRD_PARTY_NOTICES.md, README-portable.txt
  Optionally zips the folder and signs binaries when signing env is set.

.PARAMETER Arch
  Target Windows architecture ("x64" or "arm64").

.PARAMETER SkipBuild
  Skip cargo build (use an existing release binary).

.PARAMETER SkipZip
  Do not create the .zip archive.

.PARAMETER Sign
  Attempt Authenticode signing when ARIADECK_SIGN_TOOL / signtool is available.
#>
param(
    [ValidateSet("x64", "arm64")]
    [string]$Arch = "x64",
    [switch]$SkipBuild,
    [switch]$SkipZip,
    [switch]$Sign
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "windows-common.ps1")

$Root = Get-AriaDeckWorkspaceRoot -ScriptDir $PSScriptRoot
Set-Location $Root

$Version = Get-AriaDeckWorkspaceVersion -WorkspaceRoot $Root
$ArtifactName = "AriaDeck_${Version}_windows-${Arch}-portable"
$DistRoot = Join-Path $Root "dist"
$Stage = Join-Path $DistRoot $ArtifactName

$target = ""
if ($Arch -eq "arm64") {
    $target = "aarch64-pc-windows-msvc"
    $ExeSource = Join-Path $Root "target\$target\release\ariadeck-desktop.exe"
    $BridgeSource = Join-Path $Root "target\$target\release\ariadeck-bridge.exe"
} else {
    $ExeSource = Join-Path $Root "target\release\ariadeck-desktop.exe"
    $BridgeSource = Join-Path $Root "target\release\ariadeck-bridge.exe"
}

Write-Host "Workspace: $Root"
Write-Host "Version:   $Version"
Write-Host "Arch:      $Arch"
Write-Host "Stage:     $Stage"

if (-not $SkipBuild) {
    Write-Host "Building release binaries..."
    if ($target) {
        rustup target add $target
        cargo build -p ariadeck-desktop -p ariadeck-bridge --release --target $target
    } else {
        cargo build -p ariadeck-desktop -p ariadeck-bridge --release
    }
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

foreach ($binary in @($ExeSource, $BridgeSource)) {
    if (-not (Test-Path $binary)) {
        throw "Missing release binary: $binary"
    }
}

if (Test-Path $Stage) {
    Remove-Item -Recurse -Force $Stage
}
New-Item -ItemType Directory -Path $Stage | Out-Null

Copy-Item $ExeSource (Join-Path $Stage "ariadeck-desktop.exe")
Copy-Item $BridgeSource (Join-Path $Stage "ariadeck-bridge.exe")
# Empty marker enables portable data dir next to the executable.
New-Item -ItemType File -Path (Join-Path $Stage "ariadeck.portable") | Out-Null

foreach ($name in @("LICENSE", "THIRD_PARTY_NOTICES.md")) {
    $src = Join-Path $Root $name
    if (Test-Path $src) {
        Copy-Item $src (Join-Path $Stage $name)
    } else {
        Write-Warning "Missing $name — package will ship without it"
    }
}

$portableReadme = @"
AriaDeck portable ($Version)
============================

This folder is a portable build:

- Settings, profiles, cores registry, and window geometry live under .\data\
  (created on first launch) because ariadeck.portable is present.
- Delete the whole folder to fully remove the app and its portable data.
- Override the data directory with ARIADECK_DATA_DIR if needed.
- Managed aria2 is not bundled; import/link a core in Settings → Engine,
  or set ARIADECK_RPC_URL for an external engine.
- ariadeck-bridge.exe is the browser extension host. It is not registered by
  default. To enable it:
      ariadeck-bridge.exe --register --extension-id <ID>
  and to remove it again:
      ariadeck-bridge.exe --unregister
  Re-run --register after moving this folder; the registration records the
  current path. Registration is per-user (HKCU) and needs no admin rights.

Licenses: LICENSE and THIRD_PARTY_NOTICES.md in this folder.
Docs: https://github.com/AirSodaz/AriaDeck (packaging details: docs/release.md)
"@
Set-Content -Path (Join-Path $Stage "README-portable.txt") -Value $portableReadme -Encoding UTF8

Invoke-AriaDeckOptionalSign -Path (Join-Path $Stage "ariadeck-desktop.exe") -Sign:$Sign
Invoke-AriaDeckOptionalSign -Path (Join-Path $Stage "ariadeck-bridge.exe") -Sign:$Sign

if (-not $SkipZip) {
    $zip = Join-Path $DistRoot "$ArtifactName.zip"
    if (Test-Path $zip) { Remove-Item -Force $zip }
    Compress-Archive -Path $Stage -DestinationPath $zip
    Write-Host "Wrote $zip"
}

Write-Host "Portable package ready: $Stage"
