# Runs packaging/windows/install.ps1 against a local fixture HTTP server inside
# a throwaway LOCALAPPDATA, APPDATA and USERPROFILE, and a scratch HKCU key in
# place of the real Run key. Nothing outside the temp directory is touched, but
# it is meant for CI (windows-latest); it needs PowerShell 5.1 or 7 on Windows.
#
#   pwsh -NoProfile -File scripts/test-windows-installer.ps1
[CmdletBinding()]
param([string]$Installer = (Join-Path $PSScriptRoot '..\packaging\windows\install.ps1'))
$ErrorActionPreference = 'Stop'

function Check([bool]$condition, [string]$message) {
    if (-not $condition) { throw "FAIL: $message" }
    Write-Output "ok - $message"
}

$root = Join-Path ([IO.Path]::GetTempPath()) ('wordink-installer-test-' + [guid]::NewGuid().ToString('N'))
$web = Join-Path $root 'web'
New-Item -ItemType Directory -Force $web | Out-Null
$saved = @{}
foreach ($n in 'LOCALAPPDATA', 'APPDATA', 'USERPROFILE', 'WORDINK_FORCE', 'WORDINK_UNINSTALL', 'WORDINK_PURGE', 'WORDINK_YES', 'WORDINK_INSTALL_BASE', 'WORDINK_INSTALLER_NO_MAIN') {
    $saved[$n] = [Environment]::GetEnvironmentVariable($n)
}
$testKey = "HKCU:\Software\WordInkInstallerTest\$([guid]::NewGuid().ToString('N'))\Run"
$server = $null

# One release directory: wordink-<v>-windows-x64.zip plus SHA256SUMS.
function New-Release([string]$Name, [string]$Version, [switch]$WrongHash) {
    $dir = Join-Path $web $Name
    $stage = Join-Path $root "stage-$Name\wordink"
    New-Item -ItemType Directory -Force $dir, $stage | Out-Null
    Set-Content -Path (Join-Path $stage 'wordink.exe') -Value "fake wordink $Version"
    Set-Content -Path (Join-Path $stage 'LICENSE') -Value 'license'
    $zip = Join-Path $dir "wordink-$Version-windows-x64.zip"
    Compress-Archive -Path $stage -DestinationPath $zip
    $hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
    if ($WrongHash) { $hash = '0' * 64 }
    Set-Content -Path (Join-Path $dir 'SHA256SUMS') -Value "$hash  wordink-$Version-windows-x64.zip" -Encoding ascii
}

try {
    New-Release 'good' '1.2.3'
    New-Release 'newer' '1.2.4'
    New-Release 'bad' '1.2.5' -WrongHash

    # Fixture server on a free loopback port (HttpListener needs no admin there).
    $port = (Get-Random -Minimum 20000 -Maximum 40000)
    $server = Start-Job -ArgumentList $web, $port -ScriptBlock {
        param($web, $port)
        $listener = [Net.HttpListener]::new()
        $listener.Prefixes.Add("http://127.0.0.1:$port/")
        $listener.Start()
        while ($listener.IsListening) {
            $ctx = $listener.GetContext()
            $file = Join-Path $web ($ctx.Request.Url.AbsolutePath.TrimStart('/') -replace '/', '\')
            if ((Test-Path $file -PathType Leaf) -and $file.StartsWith($web)) {
                $bytes = [IO.File]::ReadAllBytes($file)
                $ctx.Response.OutputStream.Write($bytes, 0, $bytes.Length)
            } else { $ctx.Response.StatusCode = 404 }
            $ctx.Response.Close()
        }
    }
    $base = "http://127.0.0.1:$port"
    $deadline = (Get-Date).AddSeconds(20)
    while ($true) {
        try { Invoke-WebRequest -UseBasicParsing "$base/good/SHA256SUMS" | Out-Null; break }
        catch { if ((Get-Date) -gt $deadline) { throw 'fixture server did not start' }; Start-Sleep -Milliseconds 300 }
    }

    $env:LOCALAPPDATA = Join-Path $root 'Local'
    $env:APPDATA = Join-Path $root 'Roaming'
    $env:USERPROFILE = Join-Path $root 'Home'
    New-Item -ItemType Directory -Force $env:LOCALAPPDATA, $env:APPDATA, $env:USERPROFILE | Out-Null
    foreach ($n in 'WORDINK_FORCE', 'WORDINK_UNINSTALL', 'WORDINK_PURGE', 'WORDINK_YES') { Remove-Item "Env:\$n" -ErrorAction SilentlyContinue }
    $env:WORDINK_INSTALLER_NO_MAIN = '1'
    . $Installer

    # Test seams: a scratch Run key, no app launch, a fixed architecture.
    $script:arch = 'AMD64'
    $script:started = 0
    function Get-WordInkRunKey { $testKey }
    function Start-WordInkApp([string]$Exe) { $script:started++ }
    function Get-WordInkArch { $script:arch }

    function Invoke-Installer([string]$InstallBase, [string[]]$Switches = @()) {
        $env:WORDINK_INSTALL_BASE = $InstallBase
        $items = @(Invoke-WordInk $Switches 6>&1)
        [pscustomobject]@{
            Code  = [int]($items | Where-Object { $_ -is [int] } | Select-Object -Last 1)
            Lines = @($items | Where-Object { $_ -isnot [int] } | ForEach-Object { "$_" })
        }
    }

    $paths = Get-WordInkPaths
    $data = $paths.Data[0]
    $receipt = { (Get-Content -LiteralPath $paths.Receipt -TotalCount 1).Trim() }
    $runValue = { (Get-ItemProperty -Path $testKey -Name WordInk -ErrorAction SilentlyContinue).WordInk }

    # Fresh install.
    $r = Invoke-Installer "$base/good"
    Check ($r.Code -eq 0) 'fresh install exits 0'
    Check ($r.Lines.Count -le 5) "fresh install prints at most five lines ($($r.Lines.Count))"
    Check (($r.Lines -join ' ') -match 'unsigned') 'the output says the app is unsigned'
    Check (Test-Path $paths.Exe) 'wordink.exe is installed under Programs\WordInk'
    Check ((& $receipt) -eq '1.2.3') 'the receipt records 1.2.3'
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($paths.Shortcut)
    Check ($link.TargetPath -eq $paths.Exe -and $link.Arguments -eq 'app') 'the Start menu shortcut launches the installed app'
    Check ((& $runValue) -eq ('"{0}" app --hidden' -f $paths.Exe)) 'the Run value matches the app''s own login item command'
    Check ($script:started -eq 1) 'the app was started'
    Check (-not (Test-Path ($paths.Install + '.new')) -and -not (Test-Path ($paths.Install + '.old'))) 'no staging directories are left'

    # Same version again changes nothing.
    Set-Content (Join-Path $paths.Install 'marker.txt') 'x'
    $r = Invoke-Installer "$base/good"
    Check ($r.Code -eq 0 -and (Test-Path (Join-Path $paths.Install 'marker.txt'))) 're-running the same version is a no-op'
    Check ($script:started -eq 1) 'a no-op does not start the app again'

    # -Force reinstalls.
    $r = Invoke-Installer "$base/good" @('-Force')
    Check ($r.Code -eq 0 -and -not (Test-Path (Join-Path $paths.Install 'marker.txt')) -and (Test-Path $paths.Exe)) '-Force reinstalls the same version'

    # A hash mismatch fails and leaves the previous install working.
    $r = Invoke-Installer "$base/bad"
    Check ($r.Code -eq 1 -and $r.Lines[-1] -match 'SHA256') 'a hash mismatch fails with a one-line reason'
    Check ((Test-Path $paths.Exe) -and ((& $receipt) -eq '1.2.3')) 'a hash mismatch leaves the prior install in place'
    Check (-not (Test-Path ($paths.Install + '.new'))) 'a failed install leaves no staging directory'

    # A newer release upgrades.
    $r = Invoke-Installer "$base/newer"
    Check ($r.Code -eq 0 -and ((& $receipt) -eq '1.2.4')) 'a newer release upgrades'
    # An older release does not downgrade.
    $r = Invoke-Installer "$base/good"
    Check ($r.Code -eq 0 -and ((& $receipt) -eq '1.2.4')) 'an older release does not downgrade'

    # Refusals.
    $script:arch = 'ARM64'
    $r = Invoke-Installer "$base/newer" @('-Force')
    Check ($r.Code -eq 1 -and $r.Lines.Count -eq 1 -and $r.Lines[0] -match 'Windows x64' -and $r.Lines[0] -match 'github.com/duketopceo/WordInk/releases') 'ARM64 is refused with the supported-platform line'
    $script:arch = 'AMD64'
    $r = Invoke-Installer 'http://example.com/releases' @('-Force')
    Check ($r.Code -eq 1 -and $r.Lines[0] -match 'localhost') 'a non-loopback http base is refused'
    Check ((& $receipt) -eq '1.2.4') 'refusals leave the install in place'

    # Uninstall keeps data; declining the purge keeps everything; -Purge removes data.
    New-Item -ItemType Directory -Force $data | Out-Null
    Set-Content (Join-Path $data 'settings.json') '{}'
    $r = Invoke-Installer "$base/good" @('-Uninstall')
    Check ($r.Code -eq 0) 'uninstall exits 0'
    Check (-not (Test-Path $paths.Install)) 'uninstall removes the install directory'
    Check (-not (Test-Path $paths.Shortcut)) 'uninstall removes the shortcut'
    Check ($null -eq (& $runValue)) 'uninstall removes the Run value'
    Check (Test-Path (Join-Path $data 'settings.json')) 'uninstall keeps the data directory'

    $null = Invoke-Installer "$base/good"
    function Confirm-WordInkPurge([string]$Prompt) { $false }
    $r = Invoke-Installer "$base/good" @('-Uninstall', '-Purge')
    Check ((Test-Path $paths.Exe) -and (Test-Path $data)) 'declining the purge removes nothing'
    function Confirm-WordInkPurge([string]$Prompt) { $true }
    # Only the roaming data dir exists: the config dir is absent and must not fail the purge.
    $r = Invoke-Installer "$base/good" @('-Uninstall', '-Purge')
    Check ($r.Code -eq 0 -and -not (Test-Path $paths.Install) -and -not (Test-Path $data)) '-Uninstall -Purge removes the data directory even when the config directory is absent'
    # Both data dirs present, and uninstalling when nothing is installed.
    $null = Invoke-Installer "$base/good"
    foreach ($d in $paths.Data) { New-Item -ItemType Directory -Force $d | Out-Null; Set-Content (Join-Path $d 'settings.json') '{}' }
    $r = Invoke-Installer "$base/good" @('-Uninstall', '-Purge')
    Check ($r.Code -eq 0 -and -not (Test-Path $paths.Data[0]) -and -not (Test-Path $paths.Data[1])) '-Purge removes both data directories'
    $r = Invoke-Installer "$base/good" @('-Uninstall')
    Check ($r.Code -eq 0) 'uninstalling when nothing is installed succeeds'

    Write-Output 'install.ps1 passed.'
} finally {
    if ($server) { Stop-Job $server -ErrorAction SilentlyContinue; Remove-Job $server -Force -ErrorAction SilentlyContinue }
    foreach ($n in $saved.Keys) { [Environment]::SetEnvironmentVariable($n, $saved[$n]) }
    Remove-Item (Split-Path (Split-Path $testKey)) -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
}
