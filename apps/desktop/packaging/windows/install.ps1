# WordInk one-line installer for Windows (x64). Per-user, no admin rights.
#
#   Install or update:
#     irm https://github.com/duketopceo/WordInk/releases/latest/download/install.ps1 | iex
#
#   Options. Piping to iex cannot pass parameters, so use environment variables:
#     $env:WORDINK_FORCE = '1'       reinstall even when this version is installed
#     $env:WORDINK_UNINSTALL = '1'   remove WordInk (keeps your settings and history)
#     $env:WORDINK_PURGE = '1'       with uninstall: also remove settings, history, models
#     $env:WORDINK_YES = '1'         skip the purge confirmation
#     $env:WORDINK_INSTALL_BASE      download base (https, or http to localhost only)
#   Or pass switches through a script block:
#     & ([scriptblock]::Create((irm https://github.com/duketopceo/WordInk/releases/latest/download/install.ps1))) -Uninstall
#     & ([scriptblock]::Create((irm <url>))) -Uninstall -Purge
#     & ([scriptblock]::Create((irm <url>))) -Force
#
# The app is unsigned. A script install avoids the browser download mark, but
# SmartScreen may still warn the first time you open it from Explorer.
#
# Layout:  %LOCALAPPDATA%\Programs\WordInk\wordink.exe
#          Start menu shortcut "WordInk", HKCU Run value "WordInk" (the name the
#          app's own Launch at login setting uses, src/windows/login_item.rs).
# Data:    %APPDATA%\wordink and %USERPROFILE%\.config\wordink (kept by uninstall).
#
# This file deliberately has no param() block and never calls `exit`, so it is
# safe under `iex`. Tests dot-source it with WORDINK_INSTALLER_NO_MAIN=1.

$script:DefaultBase = 'https://github.com/duketopceo/WordInk/releases/latest/download'
$script:ReleasesUrl = 'https://github.com/duketopceo/WordInk/releases'

# Seams that tests override.
function Get-WordInkArch { $env:PROCESSOR_ARCHITEW6432, $env:PROCESSOR_ARCHITECTURE | Where-Object { $_ } | Select-Object -First 1 }
function Get-WordInkRunKey { 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' }
function Start-WordInkApp([string]$Exe) { Start-Process -FilePath $Exe -ArgumentList 'app' | Out-Null }
function Confirm-WordInkPurge([string]$Prompt) {
    if ($env:WORDINK_YES -eq '1') { return $true }
    (Read-Host $Prompt) -match '^(y|yes)$'
}

function Get-WordInkPaths {
    $install = Join-Path $env:LOCALAPPDATA 'Programs\WordInk'
    @{
        Install  = $install
        Exe      = Join-Path $install 'wordink.exe'
        Receipt  = Join-Path $install 'install-receipt.txt'
        Shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\WordInk.lnk'
        Data     = @((Join-Path $env:APPDATA 'wordink'), (Join-Path $env:USERPROFILE '.config\wordink'))
    }
}

function Test-WordInkBase([string]$Base) {
    $Base -match '^https://[^/\s]+' -or $Base -match '^http://(127\.0\.0\.1|localhost)(:\d+)?(/|$)'
}

# 1.2.3 and 1.2.3-rc.1; a release sorts above its own release candidates.
function Compare-WordInkVersion([string]$Left, [string]$Right) {
    $l = $Left.Trim().TrimStart('v') -split '-', 2
    $r = $Right.Trim().TrimStart('v') -split '-', 2
    $core = ([version]$l[0]).CompareTo([version]$r[0])
    if ($core -ne 0) { return $core }
    $preA = if ($l.Count -gt 1) { $l[1] } else { $null }
    $preB = if ($r.Count -gt 1) { $r[1] } else { $null }
    if (-not $preA -and -not $preB) { return 0 }
    if (-not $preA) { return 1 }
    if (-not $preB) { return -1 }
    $pa = $preA -split '\.'; $pb = $preB -split '\.'
    for ($i = 0; $i -lt [Math]::Max($pa.Count, $pb.Count); $i++) {
        if ($i -ge $pa.Count) { return -1 }
        if ($i -ge $pb.Count) { return 1 }
        $na = 0; $nb = 0
        if ([int]::TryParse($pa[$i], [ref]$na) -and [int]::TryParse($pb[$i], [ref]$nb)) {
            if ($na -ne $nb) { return $na.CompareTo($nb) }
        } elseif ($pa[$i] -ne $pb[$i]) {
            return [string]::CompareOrdinal($pa[$i], $pb[$i])
        }
    }
    0
}

function Stop-WordInkProcess([string]$InstallDir) {
    $running = @(Get-Process -Name wordink -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith($InstallDir, [StringComparison]::OrdinalIgnoreCase) })
    foreach ($p in $running) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    foreach ($p in $running) { Wait-Process -Id $p.Id -Timeout 15 -ErrorAction SilentlyContinue }
    foreach ($p in $running) {
        if (Get-Process -Id $p.Id -ErrorAction SilentlyContinue) {
            throw "Could not stop the running WordInk. Close it from the tray and run the installer again."
        }
    }
}

# Removes the install folder. A WordInk started moments ago may not report its
# path yet, and Windows can hold the image file briefly after the process
# exits, so stop it again and retry for a few seconds before giving up.
# A folder that is already gone is not an error.
function Remove-WordInkFolder([string]$Dir) {
    for ($i = 1; $i -le 20; $i++) {
        if (-not (Test-Path -LiteralPath $Dir)) { return }
        try {
            Remove-Item -LiteralPath $Dir -Recurse -Force -ErrorAction Stop
            return
        } catch {
            if ($i -eq 20) { throw }
            Stop-WordInkProcess $Dir
            Start-Sleep -Milliseconds 250
        }
    }
}

function Remove-WordInkIntegration($Paths) {
    Remove-Item -LiteralPath $Paths.Shortcut -Force -ErrorAction SilentlyContinue
    Remove-ItemProperty -Path (Get-WordInkRunKey) -Name 'WordInk' -ErrorAction SilentlyContinue
}

function Add-WordInkIntegration($Paths) {
    $dir = Split-Path $Paths.Shortcut
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($Paths.Shortcut)
    $link.TargetPath = $Paths.Exe
    $link.Arguments = 'app'
    $link.WorkingDirectory = $Paths.Install
    $link.Description = 'WordInk'
    $link.Save()
    $key = Get-WordInkRunKey
    if (-not (Test-Path $key)) { New-Item -Path $key -Force | Out-Null }
    # The same command the app writes for itself.
    Set-ItemProperty -Path $key -Name 'WordInk' -Value ('"{0}" app --hidden' -f $Paths.Exe)
}

function Invoke-WordInkUninstall($Paths, [bool]$Purge) {
    if ($Purge -and -not (Confirm-WordInkPurge 'This also deletes your WordInk settings, history and models. Type yes to continue')) {
        Write-Host 'Cancelled. Nothing was removed.'
        return
    }
    Stop-WordInkProcess $Paths.Install
    Remove-WordInkIntegration $Paths
    Remove-WordInkFolder $Paths.Install
    if ($Purge) {
        foreach ($d in $Paths.Data) {
            if (Test-Path -LiteralPath $d) { Remove-Item -LiteralPath $d -Recurse -Force -ErrorAction Stop }
        }
        Write-Host 'WordInk and its settings, history and models were removed.'
    } else {
        Write-Host ('WordInk was removed. Your settings and history are kept in {0}; add -Purge to delete them.' -f $Paths.Data[0])
    }
}

function Get-WordInkInstalledVersion($Paths) {
    if ((Test-Path $Paths.Exe) -and (Test-Path $Paths.Receipt)) {
        $v = (Get-Content -LiteralPath $Paths.Receipt -TotalCount 1).Trim()
        if ($v -match '^\d+(\.\d+){1,3}(-[0-9A-Za-z.]+)?$') { return $v }
    }
    $null
}

function Invoke-WordInkInstall($Paths, [string]$Base, [bool]$Force) {
    $arch = Get-WordInkArch
    if ($arch -ne 'AMD64' -or -not [Environment]::Is64BitOperatingSystem) {
        throw "WordInk supports Windows x64, macOS on Apple silicon and Linux x86_64 or aarch64 only; see $script:ReleasesUrl"
    }
    if (-not (Test-WordInkBase $Base)) {
        throw "WORDINK_INSTALL_BASE must be an https URL (http is allowed only for localhost); unset it to use GitHub Releases."
    }
    $Base = $Base.TrimEnd('/')
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $interactive = [Environment]::UserInteractive -and -not $env:CI
    $work = Join-Path ([IO.Path]::GetTempPath()) ('wordink-install-' + [guid]::NewGuid().ToString('N'))
    $stage = $Paths.Install + '.new'
    $backup = $Paths.Install + '.old'
    New-Item -ItemType Directory -Force -Path $work | Out-Null
    $previous = $ProgressPreference
    try {
        # 1. Find the zip in the checksum list.
        $ProgressPreference = 'SilentlyContinue'
        try { $sums = (Invoke-WebRequest -UseBasicParsing -Uri "$Base/SHA256SUMS").Content }
        catch { throw "Could not download SHA256SUMS from $Base; check your connection and try again." }
        if ($sums -is [byte[]]) { $sums = [Text.Encoding]::UTF8.GetString($sums) }
        $entry = $sums -split "`r?`n" |
            ForEach-Object { if ($_ -match '^([0-9a-fA-F]{64})\s+\*?(wordink-(\S+)-windows-x64\.zip)\s*$') { [pscustomobject]@{ Hash = $Matches[1]; Name = $Matches[2]; Version = $Matches[3] } } } |
            Select-Object -First 1
        if (-not $entry) { throw "No Windows x64 build is listed in SHA256SUMS; see $script:ReleasesUrl" }

        # 2. Decide whether to install.
        $installed = Get-WordInkInstalledVersion $Paths
        if (-not $Force -and $installed -and (Compare-WordInkVersion $entry.Version $installed) -le 0) {
            Write-Host "WordInk $installed is already installed and up to date (set WORDINK_FORCE=1 to reinstall)."
            return
        }
        Write-Host "Installing WordInk $($entry.Version) (windows-x64)..."

        # 3. Download and verify.
        $zip = Join-Path $work $entry.Name
        if ($interactive) { $ProgressPreference = 'Continue' }
        try { Invoke-WebRequest -UseBasicParsing -Uri "$Base/$($entry.Name)" -OutFile $zip }
        catch { throw "Could not download $($entry.Name); check your connection and try again." }
        $ProgressPreference = 'SilentlyContinue'
        if ((Get-FileHash -Algorithm SHA256 -LiteralPath $zip).Hash -ne $entry.Hash.ToUpperInvariant()) {
            throw "The download failed its SHA256 check, so nothing was installed; try again, and report it at $script:ReleasesUrl if it keeps failing."
        }

        # 4. Extract next to the install dir, so the final move is a rename.
        New-Item -ItemType Directory -Force -Path (Split-Path $Paths.Install) | Out-Null
        foreach ($d in $stage, $backup) { Remove-Item -LiteralPath $d -Recurse -Force -ErrorAction SilentlyContinue }
        $extract = Join-Path $work 'extract'
        try { Expand-Archive -LiteralPath $zip -DestinationPath $extract -Force }
        catch { throw "Could not unpack $($entry.Name); try again." }
        $exe = Get-ChildItem -LiteralPath $extract -Filter wordink.exe -Recurse -File | Select-Object -First 1
        if (-not $exe) { throw "The download did not contain wordink.exe, so nothing was installed; report it at $script:ReleasesUrl" }
        Move-Item -LiteralPath $exe.Directory.FullName -Destination $stage
        Set-Content -LiteralPath (Join-Path $stage 'install-receipt.txt') -Value $entry.Version -Encoding ascii
        Get-ChildItem -LiteralPath $stage -Recurse -File | Unblock-File

        # 5. Swap, restoring the previous install if anything fails.
        Stop-WordInkProcess $Paths.Install
        $hadPrevious = Test-Path $Paths.Install
        $swapped = $false
        try {
            if ($hadPrevious) { Move-Item -LiteralPath $Paths.Install -Destination $backup }
            $swapped = $true
            Move-Item -LiteralPath $stage -Destination $Paths.Install
            Add-WordInkIntegration $Paths
        } catch {
            if ($swapped) {
                Remove-Item -LiteralPath $Paths.Install -Recurse -Force -ErrorAction SilentlyContinue
                if ($hadPrevious -and (Test-Path $backup)) {
                    Move-Item -LiteralPath $backup -Destination $Paths.Install
                    try { Add-WordInkIntegration $Paths } catch { }
                } else { Remove-WordInkIntegration $Paths }
            }
            throw "Could not install into $($Paths.Install) ($($_.Exception.Message)); close WordInk and try again."
        }
        Remove-Item -LiteralPath $backup -Recurse -Force -ErrorAction SilentlyContinue

        try { Start-WordInkApp $Paths.Exe } catch { }
        Write-Host "Installed WordInk $($entry.Version). It starts at login; open it from the Start menu."
        Write-Host 'WordInk is unsigned: SmartScreen may warn the first time you open it from Explorer (More info, Run anyway).'
    } finally {
        $ProgressPreference = $previous
        Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# Returns 0 on success, 1 on failure; never exits the host.
function Invoke-WordInk([string[]]$Arguments) {
    $flags = @($Arguments | ForEach-Object { "$_".TrimStart('-').ToLowerInvariant() })
    $force = $env:WORDINK_FORCE -eq '1' -or $flags -contains 'force'
    $uninstall = $env:WORDINK_UNINSTALL -eq '1' -or $flags -contains 'uninstall'
    $purge = $env:WORDINK_PURGE -eq '1' -or $flags -contains 'purge'
    if ($flags -contains 'yes') { $env:WORDINK_YES = '1' }
    $base = if ($env:WORDINK_INSTALL_BASE) { $env:WORDINK_INSTALL_BASE } else { $script:DefaultBase }
    try {
        $paths = Get-WordInkPaths
        if ($uninstall) { Invoke-WordInkUninstall $paths $purge }
        else { Invoke-WordInkInstall $paths $base $force }
        return 0
    } catch {
        Write-Host ("WordInk failed: " + $_.Exception.Message)
        return 1
    }
}

if ($env:WORDINK_INSTALLER_NO_MAIN -ne '1') {
    $global:LASTEXITCODE = Invoke-WordInk @($args)
}
