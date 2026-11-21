#Requires -Version 5.1
<#
.SYNOPSIS
  Shared helper functions for AriaDeck Windows packaging scripts.
#>

function Get-AriaDeckWorkspaceRoot {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ScriptDir
    )
    $root = Split-Path -Parent $ScriptDir
    if (Test-Path (Join-Path $root "Cargo.toml")) {
        return $root
    }
    if (Test-Path (Join-Path $ScriptDir "Cargo.toml")) {
        return $ScriptDir
    }
    throw "Cannot locate workspace root (Cargo.toml)."
}

function Get-AriaDeckWorkspaceVersion {
    param(
        [Parameter(Mandatory = $true)]
        [string]$WorkspaceRoot
    )
    $toml = Get-Content (Join-Path $WorkspaceRoot "Cargo.toml") -Raw
    if ($toml -match '\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"') {
        return $Matches[1]
    }
    throw "Cannot determine workspace version from Cargo.toml."
}

function Invoke-AriaDeckOptionalSign {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [switch]$Sign
    )
    if (-not $Sign) { return }

    $hasThumbprint = -not [string]::IsNullOrWhiteSpace($env:ARIADECK_SIGN_CERT_THUMBPRINT)
    $hasPfx = -not [string]::IsNullOrWhiteSpace($env:ARIADECK_SIGN_PFX)
    if (-not $hasThumbprint -and -not $hasPfx) {
        Write-Warning "Signing requested but ARIADECK_SIGN_CERT_THUMBPRINT or ARIADECK_SIGN_PFX not set; skipping $Path"
        return
    }

    $tool = $env:ARIADECK_SIGN_TOOL
    if (-not $tool) {
        $command = Get-Command signtool -ErrorAction SilentlyContinue
        if ($command) { $tool = $command.Source }
    }
    if (-not $tool) {
        $kitRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
        if (Test-Path -LiteralPath $kitRoot) {
            $kitTool = Get-ChildItem -Path $kitRoot -Filter "signtool.exe" -Recurse -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -match '\\x64\\' } |
                Sort-Object FullName -Descending |
                Select-Object -First 1
            if ($kitTool) { $tool = $kitTool.FullName }
        }
    }
    if (-not $tool) {
        throw "Signing requested with configured credentials, but signtool/ARIADECK_SIGN_TOOL was not found for $Path"
    }

    $description = $env:ARIADECK_SIGN_DESCRIPTION
    if (-not $description) { $description = "AriaDeck" }

    $tempPfx = $null
    try {
        if ($hasThumbprint) {
            & $tool sign /fd SHA256 /td SHA256 /tr http://timestamp.digicert.com /sha1 $env:ARIADECK_SIGN_CERT_THUMBPRINT /d $description $Path
        } else {
            $pfxInput = $env:ARIADECK_SIGN_PFX
            $pfxPath = if (Test-Path -LiteralPath $pfxInput -PathType Leaf) {
                $pfxInput
            } else {
                $clean = ($pfxInput -replace '\s+', '')
                try {
                    $pfxBytes = [Convert]::FromBase64String($clean)
                } catch {
                    throw "ARIADECK_SIGN_PFX is neither an existing file path nor valid base64 PFX data."
                }
                $tempPfx = Join-Path ([System.IO.Path]::GetTempPath()) ("ariadeck-sign-" + [guid]::NewGuid().ToString("N") + ".pfx")
                [System.IO.File]::WriteAllBytes($tempPfx, $pfxBytes)
                $tempPfx
            }
            $arguments = @("sign", "/fd", "SHA256", "/td", "SHA256", "/tr", "http://timestamp.digicert.com", "/f", $pfxPath)
            if ($env:ARIADECK_SIGN_PFX_PASSWORD) {
                $arguments += @("/p", $env:ARIADECK_SIGN_PFX_PASSWORD)
            }
            $arguments += @("/d", $description, $Path)
            & $tool @arguments
        }

        if ($LASTEXITCODE -ne 0) {
            throw "signtool failed for $Path (exit $LASTEXITCODE)"
        }
        Write-Host "Signed $Path"
    } finally {
        if ($tempPfx -and (Test-Path -LiteralPath $tempPfx)) {
            Remove-Item -LiteralPath $tempPfx -Force -ErrorAction SilentlyContinue
        }
    }
}
