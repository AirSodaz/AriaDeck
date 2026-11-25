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

function New-AriaDeckSignContext {
    param(
        [switch]$Sign,
        [string]$ContextLabel = "artifacts"
    )
    if (-not $Sign) { return $null }

    $hasThumbprint = -not [string]::IsNullOrWhiteSpace($env:ARIADECK_SIGN_CERT_THUMBPRINT)
    $hasPfx = -not [string]::IsNullOrWhiteSpace($env:ARIADECK_SIGN_PFX)
    if (-not $hasThumbprint -and -not $hasPfx) {
        Write-Warning "Signing requested but ARIADECK_SIGN_CERT_THUMBPRINT or ARIADECK_SIGN_PFX not set; skipping $ContextLabel"
        return $null
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
        throw "Signing requested with configured credentials, but signtool/ARIADECK_SIGN_TOOL was not found for $ContextLabel"
    }

    $description = $env:ARIADECK_SIGN_DESCRIPTION
    if (-not $description) { $description = "AriaDeck" }

    $tempPfx = $null
    $pfxPath = $null
    if (-not $hasThumbprint) {
        $pfxInput = $env:ARIADECK_SIGN_PFX
        if (Test-Path -LiteralPath $pfxInput -PathType Leaf) {
            $pfxPath = $pfxInput
        } else {
            $clean = ($pfxInput -replace '\s+', '')
            try {
                $pfxBytes = [Convert]::FromBase64String($clean)
            } catch {
                throw "ARIADECK_SIGN_PFX is neither an existing file path nor valid base64 PFX data."
            }
            $tempPfx = Join-Path ([System.IO.Path]::GetTempPath()) ("ariadeck-sign-" + [guid]::NewGuid().ToString("N") + ".pfx")
            [System.IO.File]::WriteAllBytes($tempPfx, $pfxBytes)
            $pfxPath = $tempPfx
        }
    }

    return @{
        Tool          = $tool
        Description   = $description
        UseThumbprint = $hasThumbprint
        Thumbprint    = $env:ARIADECK_SIGN_CERT_THUMBPRINT
        PfxPath       = $pfxPath
        PfxPassword   = $env:ARIADECK_SIGN_PFX_PASSWORD
        TempPfx       = $tempPfx
    }
}

function Remove-AriaDeckSignContext {
    param([hashtable]$Context)
    if ($Context -and $Context.TempPfx -and (Test-Path -LiteralPath $Context.TempPfx)) {
        Remove-Item -LiteralPath $Context.TempPfx -Force -ErrorAction SilentlyContinue
    }
}

function Format-AriaDeckInnoSignToolCommand {
    param([Parameter(Mandatory = $true)][hashtable]$Context)
    # Use Inno Setup's $q quote token so paths and passwords are preserved cleanly.
    $parts = @(
        "`$q$($Context.Tool)`$q",
        "sign",
        "/fd", "SHA256",
        "/td", "SHA256",
        "/tr", "http://timestamp.digicert.com"
    )
    if ($Context.UseThumbprint) {
        $parts += @("/sha1", "`$q$($Context.Thumbprint)`$q")
    } else {
        $parts += @("/f", "`$q$($Context.PfxPath)`$q")
        if ($Context.PfxPassword) {
            $escapedPwd = $Context.PfxPassword -replace '\$', '$$$$'
            $parts += @("/p", "`$q$escapedPwd`$q")
        }
    }
    $parts += @("/d", "`$q$($Context.Description)`$q", "`$f")
    return ($parts -join " ")
}

function Invoke-AriaDeckOptionalSign {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [switch]$Sign
    )
    $context = New-AriaDeckSignContext -Sign:$Sign -ContextLabel $Path
    if (-not $context) { return }

    try {
        $arguments = @("sign", "/fd", "SHA256", "/td", "SHA256", "/tr", "http://timestamp.digicert.com")
        if ($context.UseThumbprint) {
            $arguments += @("/sha1", $context.Thumbprint)
        } else {
            $arguments += @("/f", $context.PfxPath)
            if ($context.PfxPassword) {
                $arguments += @("/p", $context.PfxPassword)
            }
        }
        $arguments += @("/d", $context.Description, $Path)
        & $context.Tool @arguments

        if ($LASTEXITCODE -ne 0) {
            throw "signtool failed for $Path (exit $LASTEXITCODE)"
        }
        Write-Host "Signed $Path"
    } finally {
        Remove-AriaDeckSignContext -Context $context
    }
}
