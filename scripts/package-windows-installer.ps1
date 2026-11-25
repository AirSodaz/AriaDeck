#Requires -Version 5.1
<#
.SYNOPSIS
  Build the AriaDeck Windows installer with Inno Setup 6+ and WiX Toolset v3.

.PARAMETER Arch
  Target Windows architecture ("x64" or "arm64").

.PARAMETER SkipBuild
  Reuse an existing release binary while staging installer inputs.

.PARAMETER SkipStage
  Reuse the existing portable staging directory.

.PARAMETER Sign
  Sign the staged executable and final installer executables when credentials exist.

.PARAMETER SkipMsi
  Skip building the WiX MSI package.

.PARAMETER SkipInno
  Skip building the Inno Setup executable installer.
#>
param(
    [ValidateSet("x64", "arm64")]
    [string]$Arch = "x64",
    [switch]$SkipBuild,
    [switch]$SkipStage,
    [switch]$Sign,
    [switch]$SkipMsi,
    [switch]$SkipInno
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "windows-common.ps1")

$Root = Get-AriaDeckWorkspaceRoot -ScriptDir $PSScriptRoot
Set-Location $Root

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

function Find-WixTool {
    param([string]$ToolName)
    $cmd = Get-Command $ToolName -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    foreach ($candidate in @(
        (Join-Path ${env:ProgramFiles(x86)} "WiX Toolset v3.11\bin\$ToolName.exe"),
        (Join-Path $env:ProgramFiles "WiX Toolset v3.11\bin\$ToolName.exe"),
        (Join-Path $env:WIX "bin\$ToolName.exe")
    )) {
        if ($candidate -and (Test-Path $candidate)) { return $candidate }
    }
    return $null
}

$Version = Get-AriaDeckWorkspaceVersion -WorkspaceRoot $Root
$Stage = Join-Path $Root "dist\AriaDeck_${Version}_windows-${Arch}-portable"
if (-not $SkipStage) {
    $arguments = @{
        Arch    = $Arch
        SkipZip = $true
    }
    if ($SkipBuild) { $arguments.SkipBuild = $true }
    if ($Sign) { $arguments.Sign = $true }
    & (Join-Path $PSScriptRoot "package-windows-portable.ps1") @arguments
}
foreach ($name in @("ariadeck-desktop.exe", "ariadeck-bridge.exe")) {
    if (-not (Test-Path (Join-Path $Stage $name))) {
        throw "Missing $name in the installer staging directory: $Stage"
    }
}

if (-not $SkipInno) {
    $compiler = Find-InnoCompiler
    $definitions = @("/DMyAppVersion=$Version", "/DAppArch=$Arch")

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

    $setup = Join-Path $Root "dist\AriaDeck_${Version}_${Arch}-setup.exe"
    if (-not (Test-Path $setup)) { throw "Missing setup executable: $setup" }
    Invoke-AriaDeckOptionalSign -Path $setup -Sign:$Sign
    Write-Host "Installer ready: $setup"
}

if (-not $SkipMsi) {
    $candle = Find-WixTool -ToolName "candle"
    $light = Find-WixTool -ToolName "light"
    if (-not $candle -or -not $light) {
        throw "WiX Toolset v3 (candle.exe and light.exe) not found. Install WiX Toolset v3 (e.g. 'choco install wixtoolset') or pass -SkipMsi."
    }

    Write-Host "Building MSI using WiX ($Arch)..."
    $objDir = Join-Path $Root "dist\wix-obj-$Arch"
    if (Test-Path $objDir) { Remove-Item -Recurse -Force $objDir }
    New-Item -ItemType Directory -Path $objDir | Out-Null
    $wixObj = Join-Path $objDir "AriaDeck.wixobj"
    $wxsPath = Join-Path $Root "packaging\windows\AriaDeck.wxs"
    $msiOut = Join-Path $Root "dist\AriaDeck_${Version}_${Arch}_en-US.msi"

    # MSI requires a strictly numeric 3-4 part version (major.minor.build[.revision])
    $msiVersion = ($Version -split '[-+]')[0]
    $parts = $msiVersion.Split('.')
    while ($parts.Count -lt 3) { $parts += "0" }
    $msiVersion = ($parts[0..2] -join '.')

    & $candle -nologo -arch $Arch "-dAppVersion=$msiVersion" "-dPlatform=$Arch" "-dSourceDir=$Stage" -out $wixObj $wxsPath
    if ($LASTEXITCODE -ne 0) { throw "WiX candle failed (exit $LASTEXITCODE)" }

    & $light -nologo -sval -out $msiOut $wixObj
    if ($LASTEXITCODE -ne 0) { throw "WiX light failed (exit $LASTEXITCODE)" }

    if (-not (Test-Path $msiOut)) { throw "Missing MSI output: $msiOut" }
    Invoke-AriaDeckOptionalSign -Path $msiOut -Sign:$Sign
    Write-Host "MSI ready: $msiOut"
    Remove-Item -Recurse -Force $objDir
}
