# Pester 5 tests for install.ps1. Everything that would touch the machine is mocked; files live in TestDrive.
# Run:  Invoke-Pester ./install.Tests.ps1 -Output Detailed

BeforeAll {
    $script:Installer = Join-Path $PSScriptRoot 'install.ps1'
    # The Windows-only cmdlets do not exist when these tests run on Linux: stub them so Mock can replace them.
    foreach ($name in 'Get-ScheduledTask', 'Register-ScheduledTask', 'Unregister-ScheduledTask', 'Start-ScheduledTask',
                      'Stop-ScheduledTask', 'Disable-ScheduledTask', 'Enable-ScheduledTask', 'New-ScheduledTaskAction', 'New-ScheduledTaskTrigger',
                      'New-ScheduledTaskSettingsSet', 'New-ScheduledTaskPrincipal', 'Get-CimInstance') {
        if (-not (Get-Command $name -ErrorAction SilentlyContinue)) { Set-Item "function:global:$name" { } }
    }
    function New-Rig {
        $root = Join-Path $TestDrive ([guid]::NewGuid().ToString('N'))
        $paths = [pscustomobject]@{
            Data           = Join-Path $root 'local\wordink'
            Startup        = Join-Path $root 'Startup'
            LegacyShortcut = Join-Path $root 'Startup\Groq Flow.lnk'
            LegacyBackup   = Join-Path $root 'local\wordink\legacy-backup'
            StateFile      = Join-Path $root 'local\wordink\legacy-state.json'
            Rollback       = Join-Path $root 'local\wordink\wordink-use-legacy.ps1'
        }
        New-Item -ItemType Directory -Force -Path $paths.Startup, $paths.Data | Out-Null
        return $paths
    }
}

Describe 'install.ps1' {
    BeforeEach {
        $env:USERNAME = 'luke'
        $script:launcher = [pscustomobject]@{ Execute = 'C:\tools\bin\wordink-dictd-tray.exe'; Arguments = '' }
    }

    It 'parses cleanly' {
        $errors = $null
        [void][System.Management.Automation.Language.Parser]::ParseFile($script:Installer, [ref]$null, [ref]$errors)
        $errors | Should -BeNullOrEmpty
    }

    It 'dry run changes nothing' {
        . $script:Installer -DryRun -Switch
        $p = New-Rig
        Set-Content $p.LegacyShortcut 'lnk'
        Mock Test-UvInstalled { $true }
        Mock Invoke-Uv { }
        Mock Register-WordInkTask { }
        Mock Start-ScheduledTask { }
        Mock Move-Item { }
        Mock Stop-Process { }
        Mock Set-Content { }
        Mock New-Item { }
        Mock Get-LegacyProcesses { [pscustomobject]@{ ProcessId = 4242; CommandLine = 'pythonw.exe -m wordink.main' } }
        Mock Get-DefaultSource { 'C:\src\sdks\python' }
        Mock Get-Requirement { 'wordink[windows] @ file:///C:/src/sdks/python' }
        Invoke-Install -Paths $p
        Should -Invoke Invoke-Uv -Times 0
        Should -Invoke Register-WordInkTask -Times 0
        Should -Invoke Start-ScheduledTask -Times 0
        Should -Invoke Move-Item -Times 0
        Should -Invoke Stop-Process -Times 0
        Should -Invoke Set-Content -Times 0
        Test-Path $p.LegacyShortcut | Should -BeTrue
    }

    It 'fails with an install hint when uv is missing' {
        . $script:Installer
        Mock Test-UvInstalled { $false }
        { Invoke-Install -Paths (New-Rig) } | Should -Throw '*uv is not installed*'
    }

    It 'installs the tool, registers the logon task and starts it' {
        . $script:Installer -Source 'wordink[windows]==0.1.0'
        Mock Test-UvInstalled { $true }
        Mock Invoke-Uv { }
        Mock Resolve-Launcher { $script:launcher }
        Mock Get-WordInkTask { $null }
        Mock Register-WordInkTask { }
        Mock Start-ScheduledTask { }
        Mock Move-Item { }
        Invoke-Install -Paths (New-Rig)
        Should -Invoke Invoke-Uv -Times 1 -ParameterFilter { $Arguments -contains 'install' -and $Arguments -contains '--force' -and $Arguments -contains 'wordink[windows]==0.1.0' }
        Should -Invoke Register-WordInkTask -Times 1
        Should -Invoke Start-ScheduledTask -Times 1
        Should -Invoke Move-Item -Times 0 # no -Switch: legacy untouched
    }

    It 'a second run registers nothing new' {
        . $script:Installer -Source 'wordink[windows]' -NoStart
        Mock Test-UvInstalled { $true }
        Mock Invoke-Uv { }
        Mock Resolve-Launcher { $script:launcher }
        Mock Get-WordInkTask { [pscustomobject]@{ Actions = @([pscustomobject]@{ Execute = 'C:\tools\bin\wordink-dictd-tray.exe'; Arguments = '' }) } }
        Mock Register-WordInkTask { }
        Mock Start-ScheduledTask { }
        Invoke-Install -Paths (New-Rig)
        Should -Invoke Register-WordInkTask -Times 0
        Should -Invoke Start-ScheduledTask -Times 0
    }

    It 'a task the rollback disabled is enabled again' {
        . $script:Installer -Source 'wordink[windows]' -NoStart
        Mock Test-UvInstalled { $true }
        Mock Invoke-Uv { }
        Mock Resolve-Launcher { $script:launcher }
        Mock Get-WordInkTask { [pscustomobject]@{ State = 'Disabled'; Actions = @([pscustomobject]@{ Execute = 'C:\tools\bin\wordink-dictd-tray.exe'; Arguments = '' }) } }
        Mock Register-WordInkTask { }
        Mock Enable-ScheduledTask { }
        Invoke-Install -Paths (New-Rig)
        Should -Invoke Register-WordInkTask -Times 0
        Should -Invoke Enable-ScheduledTask -Times 1
    }

    It 'a changed launcher re-registers the task' {
        . $script:Installer -Source 'wordink[windows]' -NoStart
        Mock Test-UvInstalled { $true }
        Mock Invoke-Uv { }
        Mock Resolve-Launcher { $script:launcher }
        Mock Get-WordInkTask { [pscustomobject]@{ Actions = @([pscustomobject]@{ Execute = 'C:\old\pythonw.exe'; Arguments = '-m x' }) } }
        Mock Register-WordInkTask { }
        Invoke-Install -Paths (New-Rig)
        Should -Invoke Register-WordInkTask -Times 1
    }

    Context 'Resolve-Launcher' {
        It 'prefers the gui-script exe' {
            . $script:Installer
            $bin = Join-Path $TestDrive 'bin1'; New-Item -ItemType Directory $bin | Out-Null
            Set-Content (Join-Path $bin 'wordink-dictd-tray.exe') 'x'
            Mock Get-UvToolBin { $bin }
            Mock Get-UvToolDir { Join-Path $TestDrive 'tools1' }
            (Resolve-Launcher).Execute | Should -Be (Join-Path $bin 'wordink-dictd-tray.exe')
        }
        It 'falls back to pythonw from the tool venv with the module argument' {
            . $script:Installer
            $bin = Join-Path $TestDrive 'bin2'; New-Item -ItemType Directory $bin | Out-Null
            $tools = Join-Path $TestDrive 'tools2'
            $scripts = Join-Path (Join-Path $tools 'wordink') 'Scripts'; New-Item -ItemType Directory -Force $scripts | Out-Null
            Set-Content (Join-Path $scripts 'pythonw.exe') 'x'
            Mock Get-UvToolBin { $bin }
            Mock Get-UvToolDir { $tools }
            $l = Resolve-Launcher
            $l.Execute | Should -Be (Join-Path $scripts 'pythonw.exe')
            $l.Arguments | Should -Be '-m wordink.desktop.win.app'
        }
        It 'fails clearly when neither exists' {
            . $script:Installer
            Mock Get-UvToolBin { Join-Path $TestDrive 'nobin' }
            Mock Get-UvToolDir { Join-Path $TestDrive 'notools' }
            { Resolve-Launcher } | Should -Throw '*Cannot find the tray launcher*'
        }
    }

    Context 'Get-Requirement' {
        It 'turns a path into a file URL requirement' {
            . $script:Installer
            $dir = Join-Path $TestDrive 'pkg'; New-Item -ItemType Directory $dir | Out-Null
            Get-Requirement -Source $dir | Should -BeLike 'wordink[[]windows] @ file:///*'
        }
        It 'accepts git URLs and plain requirements' {
            . $script:Installer
            Get-Requirement -Source 'git+https://example.com/r#subdirectory=x' | Should -Be 'wordink[windows] @ git+https://example.com/r#subdirectory=x'
            Get-Requirement -Source 'wordink[windows]==1.0' | Should -Be 'wordink[windows]==1.0'
        }
    }

    Context '-Switch and the rollback' {
        It 'moves only Groq Flow.lnk, stops the legacy process, and writes a rollback that restores it' {
            . $script:Installer
            $p = New-Rig
            Set-Content $p.LegacyShortcut 'legacy-lnk'
            $other = Join-Path $p.Startup 'OneDrive.lnk'; Set-Content $other 'other'
            Mock Get-LegacyProcesses { [pscustomobject]@{ ProcessId = 4242; CommandLine = '"C:\g\pythonw.exe" -m wordink.main' } }
            Mock Stop-Process { }
            Invoke-SwitchFromLegacy -Paths $p
            Test-Path $p.LegacyShortcut | Should -BeFalse
            Test-Path (Join-Path $p.LegacyBackup 'Groq Flow.lnk') | Should -BeTrue
            Test-Path $other | Should -BeTrue
            Should -Invoke Stop-Process -Times 1 -ParameterFilter { $Id -eq 4242 }
            $state = Get-Content $p.StateFile -Raw | ConvertFrom-Json
            $state.legacyCommandLine | Should -BeLike '*wordink.main*'
            Test-Path $p.Rollback | Should -BeTrue
            $errors = $null
            [void][System.Management.Automation.Language.Parser]::ParseFile($p.Rollback, [ref]$null, [ref]$errors)
            $errors | Should -BeNullOrEmpty

            Mock Get-ScheduledTask { $null }
            Mock Get-CimInstance { }
            Mock Start-Process { }
            & $p.Rollback
            Test-Path $p.LegacyShortcut | Should -BeTrue
            Get-Content $p.LegacyShortcut -Raw | Should -BeLike '*legacy-lnk*'
            Should -Invoke Start-Process -Times 1
        }

        It 'does nothing destructive when there is no legacy app' {
            . $script:Installer
            $p = New-Rig
            Mock Get-LegacyProcesses { }
            Mock Stop-Process { }
            Mock Move-Item { }
            Invoke-SwitchFromLegacy -Paths $p
            Should -Invoke Stop-Process -Times 0
            Should -Invoke Move-Item -Times 0
            Test-Path $p.Rollback | Should -BeFalse
            Test-Path $p.StateFile | Should -BeFalse
        }
    }

    Context '-Uninstall' {
        It 'removes the task, the rollback script and the tool, and keeps the key, vocabulary and moved shortcut' {
            . $script:Installer -Uninstall
            $p = New-Rig
            Set-Content $p.Rollback 'x'
            $vocab = Join-Path $TestDrive 'vocabulary.txt'; Set-Content $vocab 'kurultai'
            New-Item -ItemType Directory -Force $p.LegacyBackup | Out-Null
            $backup = Join-Path $p.LegacyBackup 'Groq Flow.lnk'; Set-Content $backup 'lnk'
            Mock Test-UvInstalled { $true }
            Mock Get-WordInkTask { [pscustomobject]@{ TaskName = 'WordInk Dictation' } }
            Mock Stop-ScheduledTask { }
            Mock Unregister-ScheduledTask { }
            Mock Invoke-Uv { }
            Invoke-Uninstall -Paths $p
            Should -Invoke Unregister-ScheduledTask -Times 1
            Should -Invoke Invoke-Uv -Times 1 -ParameterFilter { $Arguments -contains 'uninstall' -and $Arguments -contains 'wordink' }
            Test-Path $p.Rollback | Should -BeFalse
            Test-Path $vocab | Should -BeTrue
            Test-Path $backup | Should -BeTrue
        }
    }
}
