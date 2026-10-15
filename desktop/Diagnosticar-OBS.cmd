@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0Diagnosticar-OBS.ps1" %*
pause
