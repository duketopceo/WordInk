@echo off
REM Groq Flow - Quick Start Script
REM Run the application after installation

echo Starting Groq Flow...
echo.

REM Run the application using uv
call uv run groq-flow

if %ERRORLEVEL% NEQ 0 (
    echo.
    echo ERROR: Failed to start Groq Flow
    echo.
    echo Make sure you have:
    echo 1. Installed dependencies (run setup.bat)
    echo 2. Configured your Groq API key in .env
    echo.
    pause
    exit /b 1
)
