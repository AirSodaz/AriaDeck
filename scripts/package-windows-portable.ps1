#Requires -Version 5.1
<#
.SYNOPSIS
  Build a Windows x64 portable package for AriaDeck (RELEASE-001).

.DESCRIPTION
  Stages dist/AriaDeck-<version>-windows-x64-portable/ with:
    - ariadeck-desktop.exe
    - ariadeck.portable  (enables <exe_dir>/data)
    - LICENSE, THIRD_PARTY_NOTICES.md, README-portable.txt
  Optionally zips the folder and signs binaries when signing env is set.

.PARAMETER SkipBuild
  Skip cargo build (use an existing release binary).

.PARAMETER SkipZip
  Do not create the .zip archive.

.PARAMETER Sign
  Attempt Authenticode signing when ARIADECK_SIGN_TOOL / signtool is available.
#>
param(
    [switch]$SkipBuild,
    [switch]$SkipZip,
    [switch]$Sign
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $Root "Cargo.toml"))) {
    $Root = $PSScriptRoot
    if (-not (Test-Path (Join-Path $Root "Cargo.toml"))) {
        throw "Cannot locate workspace root (Cargo.toml)."
    }
}

Set-Location $Root

function Get-WorkspaceVersion {
    $toml = Get-Content (Join-Path $Root "Cargo.toml") -Raw
    if ($toml -match '\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"') {
        return $Matches[1]
    }
    return "0.0.0"
}

function Invoke-OptionalSign {
    param([string]$Path)
    if (-not $Sign) { return }
    $tool = $env:ARIADECK_SIGN_TOOL
    if (-not $tool) {
        $cmd = Get-Command signtool -ErrorAction SilentlyContinue
        if ($cmd) { $tool = $cmd.Source }
    }
    if (-not $tool) {
        Write-Warning "Signing requested but signtool/ARIADECK_SIGN_TOOL not found; skipping $Path"
        return
    }
    $thumb = $env:ARIADECK_SIGN_CERT_THUMBPRINT
    $desc = $env:ARIADECK_SIGN_DESCRIPTION
    if (-not $desc) { $desc = "AriaDeck" }
    if ($thumb) {
        & $tool sign /fd SHA256 /td SHA256 /tr http://timestamp.digicert.com /sha1 $thumb /d $desc $Path
    } elseif ($env:ARIADECK_SIGN_PFX) {
        $pass = $env:ARIADECK_SIGN_PFX_PASSWORD
        if ($pass) {
            & $tool sign /fd SHA256 /td SHA256 /tr http://timestamp.digicert.com /f $env:ARIADECK_SIGN_PFX /p $pass /d $desc $Path
        } else {
            & $tool sign /fd SHA256 /td SHA256 /tr http://timestamp.digicert.com /f $env:ARIADECK_SIGN_PFX /d $desc $Path
        }
    } else {
        Write-Warning "Signing requested but ARIADECK_SIGN_CERT_THUMBPRINT or ARIADECK_SIGN_PFX not set; skipping $Path"
        return
    }
    if ($LASTEXITCODE -ne 0) {
        throw "signtool failed for $Path (exit $LASTEXITCODE)"
    }
    Write-Host "Signed $Path"
}

$Version = Get-WorkspaceVersion
$ArtifactName = "AriaDeck-$Version-windows-x64-portable"
$DistRoot = Join-Path $Root "dist"
$Stage = Join-Path $DistRoot $ArtifactName
$ExeSource = Join-Path $Root "target\release\ariadeck-desktop.exe"

Write-Host "Workspace: $Root"
Write-Host "Version:   $Version"
Write-Host "Stage:     $Stage"

if (-not $SkipBuild) {
    Write-Host "Building release binary..."
    cargo build -p ariadeck-desktop --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

if (-not (Test-Path $ExeSource)) {
    throw "Missing release binary: $ExeSource"
}

if (Test-Path $Stage) {
    Remove-Item -Recurse -Force $Stage
}
New-Item -ItemType Directory -Path $Stage | Out-Null

Copy-Item $ExeSource (Join-Path $Stage "ariadeck-desktop.exe")
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

Licenses: LICENSE and THIRD_PARTY_NOTICES.md in this folder.
Docs: https://github.com/ (see repository docs/release.md)
"@
Set-Content -Path (Join-Path $Stage "README-portable.txt") -Value $portableReadme -Encoding UTF8

Invoke-OptionalSign -Path (Join-Path $Stage "ariadeck-desktop.exe")

if (-not $SkipZip) {
    $zip = Join-Path $DistRoot "$ArtifactName.zip"
    if (Test-Path $zip) { Remove-Item -Force $zip }
    Compress-Archive -Path $Stage -DestinationPath $zip
    Write-Host "Wrote $zip"
}

Write-Host "Portable package ready: $Stage"
