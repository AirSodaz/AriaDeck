#Requires -Version 5.1
<#
.SYNOPSIS
  Run the automated portion of the A3 Windows high-DPI and reparse-point gate.

.PARAMETER SkipBuild
  Reuse target\release\ariadeck-desktop.exe.

.PARAMETER ExpectedScale
  Assert the current Windows display scale before the visual pass (125 or 150).
#>
param(
    [switch]$SkipBuild,
    [ValidateSet(125, 150)]
    [int]$ExpectedScale
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if (-not $IsWindows -and $PSVersionTable.PSEdition -eq "Core") {
    throw "A3 verification must run on Windows."
}

Write-Host "Running Windows junction regression tests..."
cargo test -p ariadeck-engine windows_reparse_points_are_rejected -- --nocapture
if ($LASTEXITCODE -ne 0) { throw "Windows reparse-point tests failed." }

$Exe = Join-Path $Root "target\release\ariadeck-desktop.exe"
if (-not $SkipBuild) {
    Write-Host "Building the release desktop executable..."
    cargo build -p ariadeck-desktop --release
    if ($LASTEXITCODE -ne 0) { throw "Release build failed." }
}
if (-not (Test-Path $Exe)) { throw "Missing release executable: $Exe" }

$ImageText = [Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes($Exe))
foreach ($Marker in @("PerMonitorV2", "true/pm")) {
    if (-not $ImageText.Contains($Marker)) {
        throw "The desktop executable does not contain the expected DPI manifest marker: $Marker"
    }
}
Write-Host "DPI manifest: PerMonitorV2 (verified in $Exe)"

if ($ExpectedScale) {
    Add-Type -TypeDefinition @"
using System.Runtime.InteropServices;
public static class AriaDeckA3DpiProbe {
    [DllImport("user32.dll")]
    public static extern uint GetDpiForSystem();
}
"@
    $Dpi = [AriaDeckA3DpiProbe]::GetDpiForSystem()
    $ActualScale = [math]::Round($Dpi / 96 * 100)
    if ($ActualScale -ne $ExpectedScale) {
        throw "Expected Windows scale $ExpectedScale%, but the current system DPI is $Dpi ($ActualScale%)."
    }
    Write-Host "Display scale: $ActualScale% ($Dpi DPI)"
}

Write-Host "Automated A3 checks passed."
Write-Host "At both 125% and 150%, launch $Exe and complete the visual matrix in docs/release.md."
