# repo-zoo installer for Windows.
#
# Builds a release binary and installs it into a per-user program directory,
# adding Start Menu and (optionally) desktop shortcuts.
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

Write-Host "Building repo-zoo (release)..."
Push-Location $Root
try {
    cargo build --release
} finally {
    Pop-Location
}
if (-not (Test-Path $BuiltExe)) {
    throw "Build failed: expected $BuiltExe"
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item $BuiltExe $Exe -Force

if (-not $NoStartMenuShortcut) {
    New-Shortcut -Name "repo-zoo" -TargetPath $Exe -Directory $StartMenuDir
}
if (-not $NoDesktopShortcut) {
    New-Shortcut -Name "repo-zoo" -TargetPath $Exe -Directory $DesktopDir
}

Write-Host "Installed to $InstallDir"
Write-Host "Launch: $Exe"