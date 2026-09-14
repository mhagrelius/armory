# A release build of the Windows app, self-contained and unpackaged, under
# dotnet/publish/. No MSIX and no installer: copy the folder, run Armory.App.exe.
#
# Self-contained so the .NET runtime travels with it; the Windows App Runtime
# 1.8 is the one dependency the machine needs (winget install
# Microsoft.WindowsAppRuntime.1.8), because a framework-dependent WinUI app is
# smaller and that runtime is shared by every WinUI 3 app on the box.
#
# The compiled XAML (.xbf) and the resource index (.pri) are built but not
# copied by `dotnet publish` for an unpackaged app, and without them the
# window fails at InitializeComponent with "Cannot locate resource from
# ms-appx:///MainWindow.xaml". They are copied from the build output here.
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$out = Join-Path $PSScriptRoot 'publish'
dotnet publish Armory.App/Armory.App.csproj -c Release -r win-x64 --self-contained true -p:Platform=x64 -p:PublishReadyToRun=false -o $out --nologo
if ($LASTEXITCODE) { exit $LASTEXITCODE }
$built = Join-Path $PSScriptRoot 'Armory.App/bin/x64/Release/net10.0-windows10.0.22621.0/win-x64'
Copy-Item (Join-Path $built 'Armory.App.pri') $out -Force
Get-ChildItem $built -Recurse -Filter '*.xbf' | ForEach-Object {
    $relative = $_.FullName.Substring($built.Length + 1)
    $target = Join-Path $out $relative
    New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null
    Copy-Item $_.FullName $target -Force
}
Write-Host "published to $out"