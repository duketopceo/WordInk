@echo off
cd /d "%~dp0"
title Groq Flow - AI Talk to Text
echo ============================================================
echo Starting Groq Flow...
echo ============================================================
echo.

"%~dp0.venv\Scripts\python.exe" -m groq_flow.main --no-tray %*

if %ERRORLEVEL% NEQ 0 (
    echo.
    echo ERROR: Failed to run Groq Flow
    pause
    exit /b 1
)
