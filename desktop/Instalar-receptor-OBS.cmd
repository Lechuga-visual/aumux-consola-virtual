@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0Instalar-receptor-OBS.ps1" %*
if errorlevel 1 (
  echo.
  echo Fallo la instalacion. Para OBS normal, ejecuta como administrador.
  echo Para OBS portable: Instalar-receptor-OBS.cmd -ObsRoot "D:\OBS"
  pause
  exit /b 1
)
pause
