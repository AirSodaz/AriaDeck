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
        if ($env:ARIADECK_SIGN_PFX_PASSWORD) {
            $arguments += @("/p", $env:ARIADECK_SIGN_PFX_PASSWORD)
        }
        $arguments += @("/d", $description, $Path)
        & $tool @arguments
    } else {
        Write-Warning "Signing requested but ARIADECK_SIGN_CERT_THUMBPRINT or ARIADECK_SIGN_PFX not set; skipping $Path"
        return
    }

    if ($LASTEXITCODE -ne 0) {
        throw "signtool failed for $Path (exit $LASTEXITCODE)"
    }
    Write-Host "Signed $Path"
}
