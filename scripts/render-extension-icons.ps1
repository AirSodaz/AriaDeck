#Requires -Version 5.1
<#
.SYNOPSIS
  Rasterize the AriaDeck mark into the PNG sizes Chrome and Edge ask for.

.DESCRIPTION
  Chrome cannot use an SVG for an extension icon, so the mark has to be baked
  into PNGs. Rather than commit binaries produced by hand, this renders them from
  the app's own icon.svg in a headless browser -- the same rasterizer that will
  draw the toolbar -- so the committed PNGs are reproducible from the vector
  source and cannot drift away from the app icon.

  All four sizes come from the one source. A simplified variant for 16 and 32 px
  was tried and dropped: with the deck bars removed the letterform reads as a
  bare chevron, which looks like a different product next to the app icon. The
  full mark at 16 px is slightly soft but unmistakably the same glyph, and that
  matters more.

.PARAMETER OutputDir
  Where to write icon<size>.png. Defaults to the extension's icons directory.

.PARAMETER Browser
  Path to chrome.exe or msedge.exe. Auto-detected when omitted.
#>
param(
    [string]$OutputDir,
    [string]$Browser
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

$Source = Join-Path $Root "apps\ariadeck-desktop\assets\icon.svg"
$Sizes = @(16, 32, 48, 128)

if (-not $OutputDir) { $OutputDir = Join-Path $Root "apps\ariadeck-extension\icons" }

function Find-Browser {
    if ($Browser) {
        if (-not (Test-Path $Browser)) { throw "Browser not found: $Browser" }
        return $Browser
    }
    foreach ($candidate in @(
        (Join-Path $env:ProgramFiles "Google\Chrome\Application\chrome.exe"),
        (Join-Path ${env:ProgramFiles(x86)} "Google\Chrome\Application\chrome.exe"),
        (Join-Path $env:LOCALAPPDATA "Google\Chrome\Application\chrome.exe"),
        (Join-Path ${env:ProgramFiles(x86)} "Microsoft\Edge\Application\msedge.exe"),
        (Join-Path $env:ProgramFiles "Microsoft\Edge\Application\msedge.exe")
    )) {
        if ($candidate -and (Test-Path $candidate)) { return $candidate }
    }
    throw "No Chrome or Edge found. Pass -Browser <path to chrome.exe>."
}

$browserExe = Find-Browser
Write-Host "Renderer: $browserExe"
if (-not (Test-Path $Source)) { throw "Missing icon source: $Source" }
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$svg = Get-Content $Source -Raw
Add-Type -AssemblyName System.Drawing

# One scratch root for the wrappers and the throwaway browser profiles: a render
# must not touch (or be blocked by) the user's real Chrome profile.
$work = Join-Path ([System.IO.Path]::GetTempPath()) ("ariadeck-icons-" + [System.Guid]::NewGuid().ToString("n"))
New-Item -ItemType Directory -Force -Path $work | Out-Null
try {
    foreach ($size in $Sizes) {
        # icon.svg carries a viewBox and no intrinsic size, so CSS fixes the box
        # exactly; a transparent page keeps the rounded corners transparent.
        $html = @"
<!doctype html><meta charset="utf-8">
<style>
  html, body { margin: 0; padding: 0; background: transparent; }
  svg { display: block; width: ${size}px; height: ${size}px; }
</style>
$svg
"@
        $htmlPath = Join-Path $work "icon$size.html"
        Set-Content -Path $htmlPath -Value $html -Encoding UTF8
        $pngPath = Join-Path $OutputDir "icon$size.png"
        # A bare Windows path is not accepted as the page argument; headless then
        # screenshots about:blank. Hand it an explicit file URL.
        $url = "file:///" + ($htmlPath -replace '\\', '/')

        # --user-data-dir is per size on purpose: reusing one directory collides
        # with the previous render's singleton lock, and headless then exits 0
        # having written nothing.
        $arguments = @(
            "--headless=new"
            "--screenshot=$pngPath"
            "--window-size=$size,$size"
            "--force-device-scale-factor=1"
            "--default-background-color=00000000"
            "--hide-scrollbars"
            "--disable-gpu"
            "--no-first-run"
            "--no-default-browser-check"
            "--disable-crash-reporter"
            "--user-data-dir=$work\profile-$size"
            $url
        )
        $errorLog = Join-Path $work "chrome$size.log"

        # Start-Process -Wait, not `$out = & chrome ...`: capturing a native
        # command's output into a variable lets PowerShell return once the
        # launcher process exits, which is before the browser it spawned has
        # written the screenshot. -Wait waits on the whole process tree.
        Start-Process -FilePath $browserExe -ArgumentList $arguments -Wait -NoNewWindow `
            -RedirectStandardError $errorLog
        if (-not (Test-Path $pngPath)) {
            $log = if (Test-Path $errorLog) { Get-Content $errorLog -Raw } else { "(no output)" }
            throw "Render failed for ${size}px (no $pngPath):`n$log"
        }

        # A screenshot of the wrong size is worse than a failure: the store
        # accepts it and the browser rescales it. Verify what came out.
        $image = [System.Drawing.Image]::FromFile($pngPath)
        try { $width = $image.Width; $height = $image.Height } finally { $image.Dispose() }
        if ($width -ne $size -or $height -ne $size) {
            throw "Render produced ${width}x${height} for ${size}px: $pngPath"
        }
        Write-Host ("OK icon{0}.png ({1} bytes)" -f $size, (Get-Item $pngPath).Length)
    }
} finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
Write-Host "Icons ready: $OutputDir"
