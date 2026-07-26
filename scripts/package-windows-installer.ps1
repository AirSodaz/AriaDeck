#Requires -Version 5.1
<#
.SYNOPSIS
  Build the AriaDeck Windows installer with Inno Setup 6+.

.PARAMETER SkipBuild
  Reuse an existing release binary while staging installer inputs.

.PARAMETER SkipStage
  Reuse the existing portable staging directory.

.PARAMETER Sign
  Sign the staged executable and final setup executable when credentials exist.
#>
param(
    [switch]$SkipBuild,
    [switch]$SkipStage,
    [switch]$Sign
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

function Get-WorkspaceVersion {
    $toml = Get-Content (Join-Path $Root "Cargo.toml") -Raw
    if ($toml -match '\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"') {
        return $Matches[1]
    }
    throw "Cannot determine workspace version."
}

function Find-InnoCompiler {
    if ($env:ARIADECK_INNO_SETUP -and (Test-Path $env:ARIADECK_INNO_SETUP)) {
        return $env:ARIADECK_INNO_SETUP
    }
    $command = Get-Command iscc -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    foreach ($candidate in @(
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe"),
        (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
        (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe")
    )) {
        if ($candidate -and (Test-Path $candidate)) { return $candidate }
    }
    throw "Inno Setup 6 compiler not found. Set ARIADECK_INNO_SETUP to ISCC.exe."
}

function Invoke-OptionalSign {
    param([string]$Path)
    if (-not $Sign) { return }
    $tool = $env:ARIADECK_SIGN_TOOL
    if (-not $tool) {
        $command = Get-Command signtool -ErrorAction SilentlyContinue
        if ($command) { $tool = $command.Source }
    }
    if (-not $tool) {
        Write-Warning "Signing requested but signtool/ARIADECK_SIGN_TOOL not found; skipping $Path"
        return
    }
    $description = $env:ARIADECK_SIGN_DESCRIPTION
    if (-not $description) { $description = "AriaDeck" }
    if ($env:ARIADECK_SIGN_CERT_THUMBPRINT) {
        & $tool sign /fd SHA256 /td SHA256 /tr http://timestamp.digicert.com /sha1 $env:ARIADECK_SIGN_CERT_THUMBPRINT /d $description $Path
    } elseif ($env:ARIADECK_SIGN_PFX) {
        $arguments = @("sign", "/fd", "SHA256", "/td", "SHA256", "/tr", "http://timestamp.digicert.com", "/f", $env:ARIADECK_SIGN_PFX)
        if ($env:ARIADECK_SIGN_PFX_PASSWORD) { $arguments += @("/p", $env:ARIADECK_SIGN_PFX_PASSWORD) }
        $arguments += @("/d", $description, $Path)
        & $tool @arguments
    } else {
        Write-Warning "Signing requested but no signing certificate is configured; skipping $Path"
        return
    }
    if ($LASTEXITCODE -ne 0) { throw "signtool failed for $Path (exit $LASTEXITCODE)" }
}

$Version = Get-WorkspaceVersion
$Stage = Join-Path $Root "dist\AriaDeck-$Version-windows-x64-portable"
if (-not $SkipStage) {
    $arguments = @("-SkipZip")
    if ($SkipBuild) { $arguments += "-SkipBuild" }
    if ($Sign) { $arguments += "-Sign" }
    & (Join-Path $PSScriptRoot "package-windows-portable.ps1") @arguments
}
foreach ($name in @("ariadeck-desktop.exe", "ariadeck-bridge.exe")) {
    if (-not (Test-Path (Join-Path $Stage $name))) {
        throw "Missing $name in the installer staging directory: $Stage"
    }
}

$compiler = Find-InnoCompiler
$definitions = @("/DMyAppVersion=$Version")

# Browser bridge (D-045). Without a pinned extension ID the installer omits the
# opt-in task entirely rather than offering a registration that would accept any
# extension. Validated here so a typo fails the build, not a user's trust model.
$extensionId = $env:ARIADECK_EXTENSION_ID
if ($extensionId) {
    if ($extensionId -notmatch '^[a-p]{32}$') {
        throw "ARIADECK_EXTENSION_ID must be 32 characters in the range a-p (got '$extensionId')."
    }
    $definitions += "/DBridgeExtensionId=$extensionId"
    Write-Host "Browser bridge task enabled for extension $extensionId"
} else {
    Write-Host "ARIADECK_EXTENSION_ID not set - installer omits the browser bridge task"
}

& $compiler @definitions (Join-Path $Root "packaging\windows\AriaDeck.iss")
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed (exit $LASTEXITCODE)" }

$setup = Join-Path $Root "dist\AriaDeck-$Version-windows-x64-setup.exe"
if (-not (Test-Path $setup)) { throw "Missing setup executable: $setup" }
Invoke-OptionalSign -Path $setup
Write-Host "Installer ready: $setup"
