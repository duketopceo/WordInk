<#
.SYNOPSIS
  Install wordink-dictd (Windows dictation) for the current user, start it at logon, and optionally
  switch over from the legacy Groq Flow app with a one-command way back.

.DESCRIPTION
  Default run: install the tool with uv and register a logon Task Scheduler task. The legacy app is
  not touched. Nothing is installed system-wide and no administrator rights are needed.

  -Switch     also stop the legacy app, move its "Groq Flow.lnk" startup shortcut aside, and write
              wordink-use-legacy.ps1 (in %LOCALAPPDATA%\wordink) that undoes exactly that.
  -Uninstall  remove the task, the rollback script and the tool. Never touches the stored Groq key,
              the vocabulary, the config, or a moved legacy shortcut.
  -DryRun     print every step and change nothing.

.PARAMETER Source
  Where the wordink package comes from: a local path to sdks/python, or a pip/git requirement.
  Default: this repository's sdks/python when this script is run from a checkout, otherwise the
  GitHub repository.
#>
[CmdletBinding()]
param(
    [switch]$DryRun,
    [switch]$Uninstall,
    [switch]$Switch,
    [string]$Source,
    [switch]$NoStart
)

$ErrorActionPreference = 'Stop'

$script:TaskName = 'WordInk Dictation'
$script:ToolName = 'wordink'
$script:LegacyShortcutName = 'Groq Flow.lnk'
$script:LegacyProcessPattern = 'wordink\.main|groq-flow|groq_flow'
$script:GitSource = 'git+https://github.com/duketopceo/WordInk#subdirectory=sdks/python'

# -- environment seams (tests replace these) ---------------------------------------------------------
function Get-WordInkPaths {
    $data = Join-Path $env:LOCALAPPDATA 'wordink'
    $startup = [Environment]::GetFolderPath('Startup')
    [pscustomobject]@{
        Data           = $data
        Startup        = $startup
        LegacyShortcut = Join-Path $startup $script:LegacyShortcutName
        LegacyBackup   = Join-Path $data 'legacy-backup'
        StateFile      = Join-Path $data 'legacy-state.json'
        Rollback       = Join-Path $data 'wordink-use-legacy.ps1'
    }
}

function Write-Step {
    param([string]$Message)
    if ($DryRun) { Write-Host "[dry-run] $Message" } else { Write-Host $Message }
}

function Invoke-Change {
    # Runs a change unless -DryRun. Every side effect in this script goes through here.
    param([string]$Description, [scriptblock]$Action)
    Write-Step $Description
    if (-not $DryRun) { & $Action }
}

function Test-UvInstalled { [bool](Get-Command uv -ErrorAction SilentlyContinue) }

function Invoke-Uv {
    param([string[]]$Arguments)
    & uv @Arguments
    if ($LASTEXITCODE -ne 0) { throw "uv $($Arguments -join ' ') failed with exit code $LASTEXITCODE" }
}

function Get-UvToolBin { (& uv tool dir --bin).Trim() }
function Get-UvToolDir { (& uv tool dir).Trim() }

function Get-LegacyProcesses {
    Get-CimInstance Win32_Process -Filter "Name LIKE 'python%'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -and $_.CommandLine -match $script:LegacyProcessPattern }
}

function Get-DefaultSource {
    $local = Join-Path $PSScriptRoot '..\..\sdks\python'
    if (Test-Path (Join-Path $local 'pyproject.toml')) { return (Resolve-Path $local).Path }
    return $script:GitSource
}

function Get-Requirement {
    param([string]$Source)
    if (Test-Path $Source) {
        $uri = [System.Uri]::new((Resolve-Path $Source).Path).AbsoluteUri
        return "wordink[windows] @ $uri"
    }
    if ($Source -match '^(git\+|https?://)') { return "wordink[windows] @ $Source" }
    return $Source  # a plain requirement such as "wordink[windows]==0.1.0"
}

# -- launcher ----------------------------------------------------------------------------------------
function Resolve-Launcher {
    # The gui-script exe if uv generated it; otherwise pythonw from the tool's venv (no console window).
    $bin = Get-UvToolBin
    $exe = Join-Path $bin 'wordink-dictd-tray.exe'
    if (Test-Path $exe) { return [pscustomobject]@{ Execute = $exe; Arguments = '' } }
    $pyw = Join-Path (Join-Path (Join-Path (Get-UvToolDir) $script:ToolName) 'Scripts') 'pythonw.exe'
    if (Test-Path $pyw) { return [pscustomobject]@{ Execute = $pyw; Arguments = '-m wordink.desktop.win.app' } }
    throw "Cannot find the tray launcher after install (looked for $exe and $pyw)."
}

# -- scheduled task ----------------------------------------------------------------------------------
function Get-WordInkTask { Get-ScheduledTask -TaskName $script:TaskName -ErrorAction SilentlyContinue }

function Test-TaskMatches {
    param($Task, $Launcher)
    if (-not $Task) { return $false }
    $a = $Task.Actions | Select-Object -First 1
    return ($a.Execute -eq $Launcher.Execute) -and (([string]$a.Arguments) -eq $Launcher.Arguments)
}

function Register-WordInkTask {
    param($Launcher)
    $action = if ($Launcher.Arguments) { New-ScheduledTaskAction -Execute $Launcher.Execute -Argument $Launcher.Arguments }
              else { New-ScheduledTaskAction -Execute $Launcher.Execute }
    $trigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME
    $settings = New-ScheduledTaskSettingsSet -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) `
        -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew `
        -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
    $principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Limited
    Register-ScheduledTask -TaskName $script:TaskName -Action $action -Trigger $trigger `
        -Settings $settings -Principal $principal -Force | Out-Null
}

# -- legacy switch -----------------------------------------------------------------------------------
function Write-RollbackScript {
    param($Paths, $State)
    $legacyCmd = if ($State.legacyCommandLine) { $State.legacyCommandLine } else { '' }
    $body = @"
# Generated by install.ps1 -Switch. Brings the legacy Groq Flow app back and turns WordInk off.
`$ErrorActionPreference = 'Stop'
`$task = '$($script:TaskName)'
if (Get-ScheduledTask -TaskName `$task -ErrorAction SilentlyContinue) {
    Stop-ScheduledTask -TaskName `$task -ErrorAction SilentlyContinue
    Disable-ScheduledTask -TaskName `$task | Out-Null
}
Get-CimInstance Win32_Process -Filter "Name LIKE 'python%'" -ErrorAction SilentlyContinue |
    Where-Object { `$_.CommandLine -match 'wordink\.desktop\.win\.app' } |
    ForEach-Object { Stop-Process -Id `$_.ProcessId -Force -ErrorAction SilentlyContinue }
`$backup = '$($State.shortcutBackup)'
`$shortcut = '$($State.shortcutPath)'
if (`$backup -and (Test-Path `$backup)) { Move-Item -LiteralPath `$backup -Destination `$shortcut -Force; Write-Host "Restored `$shortcut" }
`$legacy = '$($legacyCmd -replace "'", "''")'
if (`$legacy) { Start-Process -FilePath 'cmd.exe' -ArgumentList '/c', "start `"`" `$legacy" -WindowStyle Hidden }
elseif (Test-Path `$shortcut) { Start-Process -FilePath `$shortcut }
Write-Host 'Legacy Groq Flow is back. Run install.ps1 -Switch to return to WordInk.'
"@
    Set-Content -LiteralPath $Paths.Rollback -Value $body -Encoding UTF8
}

function Invoke-SwitchFromLegacy {
    param($Paths)
    $shortcutPresent = Test-Path -LiteralPath $Paths.LegacyShortcut
    $procs = @(Get-LegacyProcesses)
    if (-not $shortcutPresent -and $procs.Count -eq 0) {
        Write-Step 'No legacy Groq Flow found (no startup shortcut, no running process): nothing to switch.'
        return
    }
    $state = [ordered]@{
        shortcutPath      = $Paths.LegacyShortcut
        shortcutBackup    = if ($shortcutPresent) { Join-Path $Paths.LegacyBackup $script:LegacyShortcutName } else { '' }
        legacyCommandLine = if ($procs.Count -gt 0) { [string]$procs[0].CommandLine } else { '' }
        recordedAt        = (Get-Date).ToString('o')
    }
    Invoke-Change "Record the legacy state in $($Paths.StateFile)" {
        New-Item -ItemType Directory -Force -Path $Paths.Data | Out-Null
        $state | ConvertTo-Json | Set-Content -LiteralPath $Paths.StateFile -Encoding UTF8
    }
    if ($shortcutPresent) {
        Invoke-Change "Move the startup shortcut '$($script:LegacyShortcutName)' to $($Paths.LegacyBackup)" {
            New-Item -ItemType Directory -Force -Path $Paths.LegacyBackup | Out-Null
            Move-Item -LiteralPath $Paths.LegacyShortcut -Destination $state.shortcutBackup -Force
        }
    }
    foreach ($p in $procs) {
        Invoke-Change "Stop the legacy process $($p.ProcessId)" { Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue }
    }
    Invoke-Change "Write the rollback script $($Paths.Rollback)" { Write-RollbackScript -Paths $Paths -State ([pscustomobject]$state) }
    Write-Step "To go back to the legacy app at any time, run: powershell -File `"$($Paths.Rollback)`""
}

# -- main --------------------------------------------------------------------------------------------
function Invoke-Install {
    param($Paths)
    if (-not (Test-UvInstalled)) {
        throw "uv is not installed. Install it first: https://docs.astral.sh/uv/getting-started/installation/ (then re-run this script)."
    }
    $src = if ($Source) { $Source } else { Get-DefaultSource }
    $req = Get-Requirement -Source $src
    Invoke-Change "Install the tool: uv tool install --force --python 3.11 `"$req`"" {
        Invoke-Uv @('tool', 'install', '--force', '--python', '3.11', $req)
    }
    if ($DryRun) {
        Write-Step "Register the logon task '$($script:TaskName)' (current user, limited rights, restart 3x at 1 min, one instance)."
    } else {
        $launcher = Resolve-Launcher
        if (Test-TaskMatches -Task (Get-WordInkTask) -Launcher $launcher) {
            Write-Step "Task '$($script:TaskName)' is already registered for $($launcher.Execute): leaving it as is."
            if ((Get-WordInkTask).State -eq 'Disabled') {
                Invoke-Change "Re-enable the task (the rollback script had disabled it)" { Enable-ScheduledTask -TaskName $script:TaskName | Out-Null }
            }
        } else {
            Invoke-Change "Register the logon task '$($script:TaskName)' running $($launcher.Execute) $($launcher.Arguments)" {
                Register-WordInkTask -Launcher $launcher
            }
        }
        if (-not $NoStart) {
            Invoke-Change "Start the task now" { Start-ScheduledTask -TaskName $script:TaskName }
        }
    }
    if ($Switch) { Invoke-SwitchFromLegacy -Paths $Paths }
    else { Write-Step 'The legacy Groq Flow app is untouched (use -Switch to replace it).' }
    Write-Step 'Next: run `wordink-dictd set-key` to store your Groq key in Windows Credential Manager, then `wordink-dictd doctor`.'
}

function Invoke-Uninstall {
    param($Paths)
    if (Get-WordInkTask) {
        Invoke-Change "Remove the task '$($script:TaskName)'" {
            Stop-ScheduledTask -TaskName $script:TaskName -ErrorAction SilentlyContinue
            Unregister-ScheduledTask -TaskName $script:TaskName -Confirm:$false
        }
    } else { Write-Step "Task '$($script:TaskName)' is not registered." }
    if (Test-Path -LiteralPath $Paths.Rollback) {
        Invoke-Change "Remove $($Paths.Rollback)" { Remove-Item -LiteralPath $Paths.Rollback -Force }
    }
    if (Test-UvInstalled) {
        Invoke-Change "Uninstall the tool: uv tool uninstall $($script:ToolName)" { Invoke-Uv @('tool', 'uninstall', $script:ToolName) }
    }
    Write-Step 'Kept: your Groq key (Credential Manager), vocabulary, config, and any legacy shortcut moved by -Switch.'
}

function Main {
    $paths = Get-WordInkPaths
    if ($Uninstall) { Invoke-Uninstall -Paths $paths } else { Invoke-Install -Paths $paths }
}

# Dot-sourcing (the tests do) loads the functions without running anything.
if ($MyInvocation.InvocationName -ne '.') { Main }
