@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0apply-local-update.ps1" -DesktopOnly
if errorlevel 1 (
    echo Update did not complete. See the message above.
    pause
    exit /b 1
)
pause
