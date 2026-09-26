# pinch-zoom installer
#
#   irm https://raw.githubusercontent.com/1000ldk/mouse-custom-own/main/install.ps1 | iex
#
# - Downloads the latest pinch-zoom.exe from GitHub Releases
#   (or copies a local build: .\install.ps1 -ExePath target\release\pinch-zoom.exe)
# - Installs it to %LOCALAPPDATA%\Programs\pinch-zoom (no admin rights needed)
# - Registers it to start at sign-in, then starts it
# Re-running updates the exe and keeps your config.toml.
param([string]$ExePath)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # makes Invoke-WebRequest much faster on PowerShell 5.1

$repo = '1000ldk/mouse-custom-own'
$dir  = Join-Path $env:LOCALAPPDATA 'Programs\pinch-zoom'
$exe  = Join-Path $dir 'pinch-zoom.exe'

New-Item -ItemType Directory -Force -Path $dir | Out-Null

# Stop a running instance so the exe can be overwritten
$running = Get-Process -Name 'pinch-zoom' -ErrorAction SilentlyContinue
if ($running) {
    Write-Host 'Stopping running pinch-zoom...'
    $running | Stop-Process -Force
    $running | Wait-Process -Timeout 5 -ErrorAction SilentlyContinue
}

if ($ExePath) {
    Write-Host "Copying $ExePath ..."
    Copy-Item -Path $ExePath -Destination $exe -Force
} else {
    $url = "https://github.com/$repo/releases/latest/download/pinch-zoom.exe"
    Write-Host "Downloading $url ..."
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $url -OutFile $exe -UseBasicParsing
}

# Start at sign-in (same as the tray menu item)
Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' `
    -Name 'pinch-zoom' -Value "`"$exe`""

Start-Process -FilePath $exe

Write-Host ''
Write-Host "Installed: $exe"
Write-Host 'pinch-zoom is running in the system tray and will start automatically at sign-in.'
Write-Host "Settings : $(Join-Path $dir 'config.toml')  (tray icon > right-click)"
