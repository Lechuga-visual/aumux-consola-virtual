@echo off
setlocal
tasklist /FI "IMAGENAME eq obs64.exe" /NH | find /I "obs64.exe" >nul
if not errorlevel 1 (
  echo Cierra OBS antes de instalar el receptor.
  pause
  exit /b 1
)
if not exist "%~dp0obs-plugin\monitor-mix-receiver.dll" (
  echo No se encuentra el receptor. Extrae todo el ZIP primero.
  pause
  exit /b 1
)
set "dest=%APPDATA%\obs-studio\plugins\monitor-mix-receiver\bin\64bit"
if not exist "%dest%" mkdir "%dest%"
set "backup=%dest%\monitor-mix-receiver-%RANDOM%-%RANDOM%.bak"
if exist "%backup%" exit /b 1
if exist "%dest%\monitor-mix-receiver.dll" (
  copy "%dest%\monitor-mix-receiver.dll" "%backup%" >nul
  if errorlevel 1 exit /b 1
)
copy /Y "%~dp0obs-plugin\monitor-mix-receiver.dll" "%dest%\monitor-mix-receiver.dll"
if errorlevel 1 (
  echo No se pudo instalar el receptor.
  pause
  exit /b 1
)
echo Receptor instalado. Abre OBS x64 32.1 o posterior.
echo Agrega la fuente Monitor Mix - master local.
pause
