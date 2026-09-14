# Reverse install.ps1. Leaves your settings and database alone.
$ErrorActionPreference = 'Stop'

$prefix = if ($env:ARMORY_PREFIX) { $env:ARMORY_PREFIX } else { Join-Path $env:LOCALAPPDATA 'Programs\Armory' }
$shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Armory.lnk'

if (Get-Process -Name 'Armory.App' -ErrorAction SilentlyContinue) {
    Write-Error 'Armory is running. Close it and run this again.'
}
if (Test-Path $prefix) { Remove-Item -Recurse -Force $prefix }
if (Test-Path $shortcut) { Remove-Item -Force $shortcut }

Write-Host 'Removed. Your settings and database are still in:'
Write-Host "  $(Join-Path $env:APPDATA 'Armory')"
Write-Host "  $(Join-Path $env:LOCALAPPDATA 'Armory')"
Write-Host 'Delete those by hand if you want them gone. The sync token is in the'
Write-Host 'Password Vault under us.hagreli.Armory.'
