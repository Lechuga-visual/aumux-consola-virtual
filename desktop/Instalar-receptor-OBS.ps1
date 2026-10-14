$ErrorActionPreference = 'Stop'
if (Get-Process obs64 -ErrorAction SilentlyContinue) {
    throw 'Cierra OBS antes de instalar el receptor.'
}
$source = Join-Path $PSScriptRoot 'obs-plugin\monitor-mix-receiver.dll'
if (-not (Test-Path $source)) { throw "No se encuentra el receptor: $source. Extrae todo el ZIP." }
$destination = Join-Path $env:APPDATA 'obs-studio\plugins\monitor-mix-receiver\bin\64bit'
New-Item -ItemType Directory -Force -Path $destination | Out-Null
$target = Join-Path $destination 'monitor-mix-receiver.dll'
if (Test-Path $target) {
    $backup = "$target.$(Get-Date -Format 'yyyyMMdd-HHmmss-fff').bak"
    Copy-Item $target $backup
    Write-Host "Copia previa conservada en $backup"
}
Copy-Item $source $target -Force
Write-Host "Receptor instalado en $target"
Write-Host 'Abre OBS x64 32.1 o posterior y agrega la fuente Monitor Mix — master local.'
