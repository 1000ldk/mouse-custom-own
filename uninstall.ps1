# pinch-zoom uninstaller
#
#   irm https://raw.githubusercontent.com/1000ldk/mouse-custom-own/main/uninstall.ps1 | iex
#
# Stops pinch-zoom, removes the sign-in entry and deletes
# %LOCALAPPDATA%\Programs\pinch-zoom (including config.toml).

$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:LOCALAPPDATA 'Programs\pinch-zoom'

$running = Get-Process -Name 'pinch-zoom' -ErrorAction SilentlyContinue
if ($running) {
    $running | Stop-Process -Force
    $running | Wait-Process -Timeout 5 -ErrorAction SilentlyContinue
}

Remove-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' `
    -Name 'pinch-zoom' -ErrorAction SilentlyContinue

if (Test-Path $dir) {
    Remove-Item -Path $dir -Recurse -Force
}

Write-Host 'pinch-zoom has been uninstalled.'
