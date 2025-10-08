# Groq Flow - Quick Start Script
# Run the application after installation

Write-Host "Starting Groq Flow..." -ForegroundColor Cyan
Write-Host ""

# Run the application using uv
uv run groq-flow

if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "ERROR: Failed to start Groq Flow" -ForegroundColor Red
    Write-Host ""
    Write-Host "Make sure you have:" -ForegroundColor Yellow
    Write-Host "1. Installed dependencies (run setup.ps1)" -ForegroundColor White
    Write-Host "2. Configured your Groq API key in .env" -ForegroundColor White
    Write-Host ""
    exit 1
}
