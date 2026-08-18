# repo-zoo installer for Windows.
#
# When run from a release archive the prebuilt repo-zoo.exe shipped next to this
# script is used and no Rust toolchain is required. When run from a repository
# checkout (no exe next to the script) a release build is performed first.
#
# Installs into a per-user program directory, adding Start Menu and (optionally)
# desktop shortcuts.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -NoDesktopShortcut
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Uninstall
#
# The config file lives in %APPDATA%\repo-zoo\config.toml and is created on the
# first run (seeded from a scan of %USERPROFILE%\code, or your home directory).
param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\repo-zoo",
    [switch]$NoDesktopShortcut,
    [switch]$NoStartMenuShortcut,
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"

$ExeName = "repo-zoo.exe"
$Root = Split-Path -Parent $PSScriptRoot
$BuiltExe = Join-Path $Root "target\release\$ExeName"
$Exe = Join-Path $InstallDir $ExeName

function New-Shortcut {
    param([string]$Name, [string]$TargetPath, [string]$Directory)
    $ws = New-Object -ComObject WScript.Shell
    $shortcut = $ws.CreateShortcut((Join-Path $Directory "$Name.lnk"))
    $shortcut.TargetPath = $TargetPath
    $shortcut.WorkingDirectory = $InstallDir
    $shortcut.IconLocation = "$TargetPath,0"
    $shortcut.Description = "repo-zoo code project launcher"
    $shortcut.Save()
}

function Remove-Shortcut {
    param([string]$Name, [string]$Directory)
    $path = Join-Path $Directory "$Name.lnk"
    if (Test-Path $path) { Remove-Item $path -Force }
}

$StartMenuDir = [Environment]::GetFolderPath("StartMenu") + "\Programs"
$DesktopDir = [Environment]::GetFolderPath("Desktop")

if ($Uninstall) {
    Remove-Shortcut -Name "repo-zoo" -Directory $StartMenuDir
    Remove-Shortcut -Name "repo-zoo" -Directory $DesktopDir
    if (Test-Path $InstallDir) {
        Remove-Item $InstallDir -Recurse -Force
    }
    Write-Host "repo-zoo uninstalled."
    exit 0
}

# Use the prebuilt exe shipped next to this script (release archive) and skip
# the build; a repository checkout builds a release instead.
$PrebuiltExe = Join-Path $PSScriptRoot $ExeName
if (Test-Path $PrebuiltExe) {
    $SourceExe = $PrebuiltExe
} else {
    Write-Host "Building repo-zoo (release)..."
    Push-Location $Root
    try {
        cargo build --release
    } finally {
        Pop-Location
    }
    $SourceExe = $BuiltExe
}
if (-not (Test-Path $SourceExe)) {
    throw "Expected repo-zoo binary at $SourceExe"
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item $SourceExe $Exe -Force

if (-not $NoStartMenuShortcut) {
    New-Shortcut -Name "repo-zoo" -TargetPath $Exe -Directory $StartMenuDir
}
if (-not $NoDesktopShortcut) {
    New-Shortcut -Name "repo-zoo" -TargetPath $Exe -Directory $DesktopDir
}

Write-Host "Installed to $InstallDir"
Write-Host "Launch: $Exe"