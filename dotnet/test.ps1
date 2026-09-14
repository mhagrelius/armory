# The gate for the C# half: format, build with warnings as errors, test.
# Run it, not bare `dotnet test`.
#
# The tests are run as the executable xunit v3 builds, because `dotnet test`
# on the .NET 10 SDK insists on Microsoft.Testing.Platform mode and the
# opt-in is not honoured from the CLI here. The executable is that platform.
#
# One gate at a time: two builds of the same tree at once fight over obj/,
# and the mutex is what lets several sessions share one checkout.
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$gate = New-Object System.Threading.Mutex($false, 'Global\ArmoryDotnetGate')
[void]$gate.WaitOne()
try {
    dotnet format --verify-no-changes Armory.slnx
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
    dotnet build Armory.slnx -c Debug --nologo -v q
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
    dotnet Armory.Core.Tests/bin/Debug/net10.0/Armory.Core.Tests.dll
    exit $LASTEXITCODE
} finally {
    $gate.ReleaseMutex()
}
