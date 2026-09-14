# Copy the collector addon into every WoW client under one install. The
# Windows half of ../install-addon.sh.
#
# Finds the install the way Armory does, or takes a path — to the folder that
# holds `_retail_`, `_classic_era_` and the rest, or to any one of those:
#   pwsh dotnet/install-addon.ps1
#   pwsh dotnet/install-addon.ps1 "C:\Program Files (x86)\World of Warcraft"
#   pwsh dotnet/install-addon.ps1 "C:\Program Files (x86)\World of Warcraft\_retail_"
#
# Every client folder that has a WTF directory gets a copy. The same addon
# runs on all of them; what differs is which APIs answer, and the addon asks
# before calling.
param([string]$Root)
$ErrorActionPreference = 'Stop'

$addon = 'Armory_Collector'
$source = Join-Path (Split-Path $PSScriptRoot -Parent) 'addon' $addon

# The places Settings.WowSearchPaths looks, in the same order: both Program
# Files folders, then a Games folder on the home drive. An install is
# identified by .build.info, which the launcher writes at the install root.
function Find-Wow {
    $candidates = @()
    foreach ($folder in 'ProgramFilesX86', 'ProgramFiles') {
        $programs = [Environment]::GetFolderPath($folder)
        if ($programs) { $candidates += Join-Path $programs 'World of Warcraft' }
    }
    $candidates += Join-Path (Split-Path $HOME -Qualifier) 'Games\World of Warcraft'
    foreach ($candidate in $candidates) {
        if (Test-Path -PathType Leaf (Join-Path $candidate '.build.info')) { return $candidate }
    }
    return $null
}

if (-not $Root) { $Root = Find-Wow }
if (-not $Root) {
    [Console]::Error.WriteLine('Could not find a WoW install. Pass the path to World of Warcraft:')
    [Console]::Error.WriteLine('  pwsh dotnet/install-addon.ps1 "C:\Program Files (x86)\World of Warcraft"')
    exit 1
}
# A client folder was passed rather than the install: step up to the install,
# so the other clients beside it are found too.
$parent = Split-Path $Root -Parent
if ((Test-Path -PathType Container (Join-Path $Root 'WTF')) -and $parent -and (Test-Path -PathType Leaf (Join-Path $parent '.build.info'))) {
    $Root = $parent
}
if (-not (Test-Path -PathType Leaf (Join-Path $Root '.build.info'))) {
    [Console]::Error.WriteLine("$Root does not look like a WoW install (no .build.info).")
    exit 1
}

# Every client's version, from .build.info: one row per installed product,
# with the version as e.g. 12.0.7.68887. Collected first, because each copy
# of the .toc lists every installed client's interface number.
#
# WoW greys out and refuses to load an addon whose Interface number does not
# match the client, unless "Load out of date AddOns" is ticked — and a
# data-capture addon that silently does not load looks exactly like Armory
# being broken. The .toc takes a comma-separated list, and a client picks the
# entry that is its own, so one file serves the retail client and the Classic
# ones beside it. The interface number is major*10000 + minor*100 + patch.
$interfaces = @()
$versions = @()
foreach ($line in (Get-Content (Join-Path $Root '.build.info') | Select-Object -Skip 1)) {
    $row = $line -split '\|'
    $version = if ($row.Count -gt 12) { $row[12] } else { '' }
    if ($version -match '^(\d+)\.(\d+)\.(\d+)') {
        $interfaces += '{0}{1:D2}{2:D2}' -f [int]$Matches[1], [int]$Matches[2], [int]$Matches[3]
        $versions += $version
    }
}
$interfaceList = $interfaces -join ', '
$versionList = $versions -join ', '

$installed = 0
foreach ($client in Get-ChildItem -Directory -Path $Root -Filter '_*_') {
    if (-not ((Test-Path -PathType Container (Join-Path $client.FullName 'WTF')) -or (Test-Path -PathType Container (Join-Path $client.FullName 'Interface')))) {
        continue
    }
    $target = Join-Path $client.FullName 'Interface' 'AddOns' $addon
    New-Item -ItemType Directory -Force $target | Out-Null
    # Every Lua file in the folder, not a named list. The .toc says which of
    # them the client loads; an installer that names them too is a second list
    # to keep in step, and the failure when it drifts is a file that silently
    # never loads.
    Copy-Item -Force (Join-Path $source "$addon.toc") $target
    Copy-Item -Force (Join-Path $source '*.lua') $target
    if ($interfaceList) {
        # Rewritten as bytes rather than through Set-Content, which would
        # turn the repo's LF line endings into CRLF on every install.
        $toc = Join-Path $target "$addon.toc"
        $text = [IO.File]::ReadAllText($toc)
        $text = [regex]::Replace($text, '(?m)^## Interface:[^\r\n]*', "## Interface: $interfaceList")
        [IO.File]::WriteAllText($toc, $text)
    }
    Write-Host "Installed $addon to $target"
    $installed++
}

if ($installed -eq 0) {
    [Console]::Error.WriteLine("No client folder under $Root has a WTF directory; nothing installed.")
    exit 1
}
if ($interfaceList) {
    Write-Host "Matched your clients: WoW $versionList (interface $interfaceList)"
}
Write-Host ''
Write-Host 'Restart WoW if it is running, then log in once and log out — the addon'
Write-Host 'writes its file on logout, which is the only time WoW saves one.'
