param(
    [string[]]$PortableDirs = @(
        "$env:USERPROFILE\\OneDrive\\Desktop\\Soziopolis Reader Portable",
        "C:\\Soziopolis Reader Portable"
    ),
    [switch]$SkipBuild,
    [switch]$NoDesktopShortcut
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$releaseExe = Join-Path $repoRoot "target\\release\\soziopolis_lingq_tool.exe"
$iconSource = Join-Path $repoRoot "assets\\soziopolis-hires.ico"
$desktopShortcutPath = Join-Path ([Environment]::GetFolderPath("Desktop")) "Soziopolis Reader.lnk"

if (-not $SkipBuild) {
    Push-Location $repoRoot
    try {
        cargo build --release
    }
    finally {
        Pop-Location
    }
}

if (-not (Test-Path $releaseExe)) {
    throw "Release executable not found at $releaseExe"
}

if (-not (Test-Path $iconSource)) {
    throw "Icon file not found at $iconSource"
}

$portableReadme = @"
Soziopolis Reader Portable

Launch:
- Soziopolis Reader.exe

Portable data:
- data\\soziopolis_lingq_tool\\settings.json
- data\\soziopolis_lingq_tool\\soziopolis_lingq_tool.db
- data\\soziopolis_lingq_tool\\logs\\soziopolis-reader.log

Notes:
- This folder is self-contained for app binaries, icon, settings, database, and logs.
- On Windows, the LingQ token is stored in Windows Credential Manager, so LingQ may need to be reconnected once on a new PC.
"@

$builtTargets = @()
foreach ($portableDir in $PortableDirs) {
    if ([string]::IsNullOrWhiteSpace($portableDir)) {
        continue
    }

    $null = New-Item -ItemType Directory -Path $portableDir -Force
    $null = New-Item -ItemType Directory -Path (Join-Path $portableDir "data") -Force

    Copy-Item -LiteralPath $releaseExe -Destination (Join-Path $portableDir "Soziopolis Reader.exe") -Force
    Remove-Item -LiteralPath (Join-Path $portableDir "soziopolis_lingq_tool.exe") -Force -ErrorAction SilentlyContinue
    Copy-Item -LiteralPath $iconSource -Destination (Join-Path $portableDir "soziopolis-hires.ico") -Force
    Set-Content -LiteralPath (Join-Path $portableDir "README.txt") -Value $portableReadme -Encoding ASCII

    $builtTargets += $portableDir
}

if (-not $NoDesktopShortcut -and $builtTargets.Count -gt 0) {
    $desktopTarget = $builtTargets[0]
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($desktopShortcutPath)
    $shortcut.TargetPath = Join-Path $desktopTarget "Soziopolis Reader.exe"
    $shortcut.WorkingDirectory = $desktopTarget
    $shortcut.IconLocation = "$(Join-Path $desktopTarget 'soziopolis-hires.ico'),0"
    $shortcut.Description = "Launch Soziopolis Reader"
    $shortcut.Save()
}

$builtTargets | ForEach-Object { Write-Output "Portable bundle refreshed: $_" }
if (-not $NoDesktopShortcut -and $builtTargets.Count -gt 0) {
    Write-Output "Desktop shortcut refreshed: $desktopShortcutPath"
}
