param(
    [string]$Version = "",
    [string]$OutputDir = "",
    [string]$IsccPath = "",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

function Resolve-IsccPath {
    param([string]$ExplicitPath)

    if (-not [string]::IsNullOrWhiteSpace($ExplicitPath)) {
        if (Test-Path $ExplicitPath) {
            return (Resolve-Path $ExplicitPath).Path
        }
        throw "The specified Inno Setup compiler was not found at $ExplicitPath"
    }

    if ($env:INNO_SETUP_COMPILER -and (Test-Path $env:INNO_SETUP_COMPILER)) {
        return (Resolve-Path $env:INNO_SETUP_COMPILER).Path
    }

    $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    $commonPaths = @(
        "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
        "C:\Program Files\Inno Setup 6\ISCC.exe"
    )

    foreach ($path in $commonPaths) {
        if (Test-Path $path) {
            return $path
        }
    }

    throw @"
Inno Setup 6 was not found.

Install it from:
https://jrsoftware.org/isinfo.php

Then rerun this script, or pass -IsccPath "C:\Path\To\ISCC.exe"
"@
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$cargoToml = Join-Path $repoRoot "Cargo.toml"
$releaseExe = Join-Path $repoRoot "target\release\soziopolis_lingq_tool.exe"
$iconSource = Join-Path $repoRoot "assets\soziopolis-hires.ico"
$issFile = Join-Path $repoRoot "installer\SoziopolisReader.iss"
$resolvedOutputDir = if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    Join-Path $repoRoot "dist"
} else {
    $OutputDir
}
$stageDir = Join-Path $resolvedOutputDir "installer-staging"

if (-not $SkipBuild) {
    Push-Location $repoRoot
    try {
        cargo build --release
    }
    finally {
        Pop-Location
    }
}

if ([string]::IsNullOrWhiteSpace($Version)) {
    if (-not (Test-Path $cargoToml)) {
        throw "Cargo.toml not found at $cargoToml"
    }
    $cargoTomlText = Get-Content $cargoToml -Raw
    $versionMatch = [regex]::Match($cargoTomlText, 'version\s*=\s*"([^"]+)"')
    if (-not $versionMatch.Success) {
        throw "Could not determine the package version from $cargoToml"
    }
    $Version = $versionMatch.Groups[1].Value
}

if (-not (Test-Path $releaseExe)) {
    throw "Release executable not found at $releaseExe"
}

if (-not (Test-Path $iconSource)) {
    throw "Icon file not found at $iconSource"
}

if (-not (Test-Path $issFile)) {
    throw "Installer definition not found at $issFile"
}

$null = New-Item -ItemType Directory -Path $resolvedOutputDir -Force
if (Test-Path $stageDir) {
    Remove-Item -LiteralPath $stageDir -Recurse -Force
}
$null = New-Item -ItemType Directory -Path $stageDir -Force

Copy-Item -LiteralPath $releaseExe -Destination (Join-Path $stageDir "Soziopolis Reader.exe") -Force
Copy-Item -LiteralPath $iconSource -Destination (Join-Path $stageDir "soziopolis-hires.ico") -Force

$installerReadme = @"
Soziopolis Reader $Version

Installed application path:
- {app}\Soziopolis Reader.exe

Data storage:
- %LOCALAPPDATA%\soziopolis_lingq_tool\

Notes:
- LingQ tokens are stored in Windows Credential Manager.
- Uninstall from Apps & Features or the Start Menu uninstall entry.
"@
Set-Content -LiteralPath (Join-Path $stageDir "README.txt") -Value $installerReadme -Encoding ASCII

$resolvedIsccPath = Resolve-IsccPath -ExplicitPath $IsccPath

& $resolvedIsccPath `
    "/DAppVersion=$Version" `
    "/DStageDir=$stageDir" `
    "/DOutputDir=$resolvedOutputDir" `
    $issFile

$installerOutput = Join-Path $resolvedOutputDir "SoziopolisReaderSetup-$Version.exe"
if (-not (Test-Path $installerOutput)) {
    throw "Expected installer output not found at $installerOutput"
}

Write-Output "Installer created: $installerOutput"
