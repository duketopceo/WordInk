# Setup Groq Flow to run on Windows startup
# This script creates a startup shortcut in the Windows Startup folder

param(
    [switch]$Remove
)

$ErrorActionPreference = "Stop"

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  Groq Flow - Startup Configuration" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# Get current directory
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectDir = $scriptDir

# Get startup folder path
$startupFolder = [Environment]::GetFolderPath('Startup')
$shortcutPath = Join-Path $startupFolder "Groq Flow.lnk"

if ($Remove) {
    # Remove from startup
    Write-Host "Removing Groq Flow from startup..." -ForegroundColor Yellow
    
    if (Test-Path $shortcutPath) {
        Remove-Item $shortcutPath -Force
        Write-Host "✅ Groq Flow removed from startup!" -ForegroundColor Green
        Write-Host ""
        Write-Host "The application will no longer start automatically on login." -ForegroundColor Gray
    } else {
        Write-Host "⚠️  Groq Flow startup shortcut not found." -ForegroundColor Yellow
    }
} else {
    # Add to startup
    Write-Host "Setting up Groq Flow to start on Windows login..." -ForegroundColor Cyan
    Write-Host ""
    
    # Check if uv is installed
    try {
        $uvPath = (Get-Command uv -ErrorAction Stop).Path
        Write-Host "✅ Found uv at: $uvPath" -ForegroundColor Green
    } catch {
        Write-Host "❌ Error: uv not found in PATH!" -ForegroundColor Red
        Write-Host "Please run setup.ps1 first to install dependencies." -ForegroundColor Yellow
        exit 1
    }
    
    # Create startup batch file
    $batchPath = Join-Path $projectDir "start_hidden.bat"
    $batchContent = @"
@echo off
cd /d "$projectDir"
start /min "" uv run groq-flow
"@
    
    Set-Content -Path $batchPath -Value $batchContent -Encoding ASCII
    Write-Host "✅ Created startup batch file" -ForegroundColor Green
    
    # Create shortcut
    $WshShell = New-Object -ComObject WScript.Shell
    $Shortcut = $WshShell.CreateShortcut($shortcutPath)
    $Shortcut.TargetPath = "cmd.exe"
    $Shortcut.Arguments = "/c `"$batchPath`""
    $Shortcut.WorkingDirectory = $projectDir
    $Shortcut.WindowStyle = 7  # Minimized
    $Shortcut.Description = "Groq Flow - AI Speech-to-Text"
    $Shortcut.Save()
    
    Write-Host "✅ Created startup shortcut" -ForegroundColor Green
    Write-Host ""
    Write-Host "========================================" -ForegroundColor Green
    Write-Host "  ✅ Setup Complete!" -ForegroundColor Green
    Write-Host "========================================" -ForegroundColor Green
    Write-Host ""
    Write-Host "Groq Flow will now start automatically when you log in to Windows." -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Shortcut location:" -ForegroundColor Gray
    Write-Host "  $shortcutPath" -ForegroundColor Gray
    Write-Host ""
    Write-Host "To remove from startup, run:" -ForegroundColor Gray
    Write-Host "  .\setup_autostart.ps1 -Remove" -ForegroundColor Yellow
    Write-Host ""
}

Write-Host "Press any key to continue..."
$null = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")
