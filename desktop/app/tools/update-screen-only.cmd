@echo off
setlocal
if "%~1"=="" (
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0apply-local-update.ps1" -FirmwareOnly
) else (
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0apply-local-update.ps1" -FirmwareOnly -Port "%~1"
)
if errorlevel 1 (
    echo Screen update did not complete. See the message above.
    pause
    exit /b 1
)
pause
