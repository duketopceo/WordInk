# Installation and Setup Script for Groq Flow
# Run this script to set up the application

Write-Host "============================================" -ForegroundColor Cyan
Write-Host "  Groq Flow - Installation Script" -ForegroundColor Cyan
Write-Host "============================================" -ForegroundColor Cyan
Write-Host ""

# Check if uv is installed
Write-Host "Checking for uv..." -ForegroundColor Yellow
if (!(Get-Command uv -ErrorAction SilentlyContinue)) {
    Write-Host "ERROR: uv is not installed!" -ForegroundColor Red
    Write-Host "Please install uv first: https://github.com/astral-sh/uv" -ForegroundColor Red
    exit 1
}

Write-Host "✓ uv found" -ForegroundColor Green
Write-Host ""

# Sync dependencies
Write-Host "Installing dependencies..." -ForegroundColor Yellow
uv sync

if ($LASTEXITCODE -ne 0) {
    Write-Host "ERROR: Failed to install dependencies" -ForegroundColor Red
    exit 1
}

Write-Host "✓ Dependencies installed" -ForegroundColor Green
Write-Host ""

# Create .env file if it doesn't exist
if (!(Test-Path .env)) {
    Write-Host "Creating .env file from template..." -ForegroundColor Yellow
    Copy-Item .env.example .env
    Write-Host "✓ .env file created" -ForegroundColor Green
    Write-Host ""
    Write-Host "IMPORTANT: Please edit .env and add your Groq API key!" -ForegroundColor Yellow
    Write-Host "Get your API key from: https://console.groq.com/" -ForegroundColor Cyan
    Write-Host ""
} else {
    Write-Host "✓ .env file already exists" -ForegroundColor Green
    Write-Host ""
}

# Check for Groq API key
Write-Host "Checking Groq API key..." -ForegroundColor Yellow
$envContent = Get-Content .env -Raw
if ($envContent -match "GROQ_API_KEY=your_groq_api_key_here" -or $envContent -notmatch "GROQ_API_KEY=\w+") {
    Write-Host "WARNING: Groq API key not configured!" -ForegroundColor Red
    Write-Host "Please edit .env and add your Groq API key" -ForegroundColor Yellow
    Write-Host ""
} else {
    Write-Host "✓ Groq API key configured" -ForegroundColor Green
    Write-Host ""
}

Write-Host "============================================" -ForegroundColor Cyan
Write-Host "  Installation Complete!" -ForegroundColor Green
Write-Host "============================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "To run Groq Flow:" -ForegroundColor Cyan
Write-Host "  uv run groq-flow" -ForegroundColor White
Write-Host ""
Write-Host "Or activate the virtual environment:" -ForegroundColor Cyan
Write-Host "  .venv\Scripts\Activate.ps1" -ForegroundColor White
Write-Host "  groq-flow" -ForegroundColor White
Write-Host ""
