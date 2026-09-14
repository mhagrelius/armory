# `dotnet format` under the same mutex as the gate, so a formatter pass from
# one session never races another session's build.
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$gate = New-Object System.Threading.Mutex($false, 'Global\ArmoryDotnetGate')
[void]$gate.WaitOne()
try {
    dotnet format Armory.slnx
    exit $LASTEXITCODE
} finally {
    $gate.ReleaseMutex()
}
