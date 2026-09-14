# Publish in release and install under %LOCALAPPDATA%\Programs\Armory, with a
# Start Menu shortcut. The Windows half of ../install.sh; uninstall.ps1
# reverses it and leaves your settings and database alone.
#
# No MSIX and no installer: the published folder is the installation. It
# carries its own .NET; the Windows App Runtime 1.8 is the one thing the
# machine needs (winget install Microsoft.WindowsAppRuntime.1.8).
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

$prefix = if ($env:ARMORY_PREFIX) { $env:ARMORY_PREFIX } else { Join-Path $env:LOCALAPPDATA 'Programs\Armory' }
$programs = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$shortcut = Join-Path $programs 'Armory.lnk'

Write-Host '==> publish.ps1'
pwsh -NoProfile -File ./publish.ps1
if ($LASTEXITCODE) { exit $LASTEXITCODE }

Write-Host "==> installing to $prefix"
if (Get-Process -Name 'Armory.App' -ErrorAction SilentlyContinue) {
    Write-Error 'Armory is running. Close it and run this again.'
}
# Replace the folder whole, so a file the new build no longer ships does not
# linger from the old one. Nothing of yours is in here: settings live under
# %APPDATA%\Armory, the store and image cache under %LOCALAPPDATA%\Armory.
if (Test-Path $prefix) { Remove-Item -Recurse -Force $prefix }
New-Item -ItemType Directory -Force $prefix | Out-Null
Copy-Item -Recurse -Force (Join-Path $PSScriptRoot 'publish\*') $prefix

Write-Host "==> $shortcut"
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($shortcut)
$link.TargetPath = Join-Path $prefix 'Armory.App.exe'
$link.WorkingDirectory = $prefix
$link.Description = 'A World of Warcraft companion'
$link.Save()

Write-Host ''
Write-Host 'Installed. Find Armory in the Start Menu, or run:'
Write-Host "  $(Join-Path $prefix 'Armory.App.exe')"
Write-Host ''
Write-Host 'The collector addon is optional and installed separately:'
Write-Host '  pwsh dotnet/install-addon.ps1'
