# Builds wordink-setup-<version>-windows-<x64|arm64>.exe, the per-user Windows
# installer (packaging/windows/wordink.iss), around an already-built release
# wordink.exe, and prints its path. Installs Inno Setup 6 when ISCC is missing.
# The installer embeds the executable byte for byte. The Desktop release
# workflow publishes it as WordInk-Setup-windows-x64.exe.
#
#   scripts/build-windows-installer.ps1 -Executable target\release\wordink.exe `
#     -Version 1.2.3 -Arch x86_64 -OutDir dist
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string]$Executable,
    [Parameter(Mandatory)] [string]$Version,
    [Parameter(Mandatory)] [ValidateSet('x86_64', 'aarch64')] [string]$Arch,
    [Parameter(Mandatory)] [string]$OutDir
)
$ErrorActionPreference = 'Stop'

# A release (1.2.3), a release candidate (1.2.3-rc.1) or a workflow dry run
# (0.0.0-dryrun.42).
if ($Version -notmatch '^\d+\.\d+\.\d+(-(rc|dryrun)\.\d+)?$') { throw "Expected a release version, not $Version." }
$Executable = (Resolve-Path $Executable).Path
New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$label = if ($Arch -eq 'x86_64') { 'x64' } else { 'arm64' }
$script = Join-Path (Split-Path -Parent $PSScriptRoot) 'packaging\windows\wordink.iss'

function Find-Iscc {
    $candidates = @(
        (Get-Command ISCC.exe -ErrorAction SilentlyContinue | ForEach-Object Source),
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    )
    $candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
}

$iscc = Find-Iscc
if (-not $iscc) {
    if (Get-Command choco.exe -ErrorAction SilentlyContinue) {
        choco install innosetup --yes --no-progress | Out-Host
    } else {
        $setup = Join-Path $env:TEMP 'innosetup.exe'
        Invoke-WebRequest -Uri 'https://jrsoftware.org/download.php/is.exe' -OutFile $setup -UseBasicParsing
        Start-Process $setup -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER' -Wait
    }
    $iscc = Find-Iscc
    if (-not $iscc) { throw 'Inno Setup could not be installed.' }
}

& $iscc /Qp "/DAppVersion=$Version" "/DArch=$label" "/DExecutable=$Executable" "/DOutputDir=$OutDir" $script
if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE." }
$installer = Join-Path $OutDir "wordink-setup-$Version-windows-$label.exe"
if (-not (Test-Path $installer)) { throw "ISCC did not produce $installer." }
Write-Output $installer
