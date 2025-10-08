@echo off
REM Installation and Setup Script for Groq Flow
REM Run this script to set up the application

echo ============================================
echo   Groq Flow - Installation Script
echo ============================================
echo.

REM Check if uv is installed
echo Checking for uv...
where uv >nul 2>nul
if %ERRORLEVEL% NEQ 0 (
    echo ERROR: uv is not installed!
    echo Please install uv first: https://github.com/astral-sh/uv
    exit /b 1
)

echo [OK] uv found
echo.

REM Sync dependencies
echo Installing dependencies...
call uv sync

if %ERRORLEVEL% NEQ 0 (
    echo ERROR: Failed to install dependencies
    exit /b 1
)

echo [OK] Dependencies installed
echo.

REM Create .env file if it doesn't exist
if not exist .env (
    echo Creating .env file from template...
    copy .env.example .env
    echo [OK] .env file created
    echo.
    echo IMPORTANT: Please edit .env and add your Groq API key!
    echo Get your API key from: https://console.groq.com/
    echo.
) else (
    echo [OK] .env file already exists
    echo.
)

echo ============================================
echo   Installation Complete!
echo ============================================
echo.
echo To run Groq Flow:
echo   uv run groq-flow
echo.
echo Or activate the virtual environment:
echo   .venv\Scripts\activate.bat
echo   groq-flow
echo.

pause
