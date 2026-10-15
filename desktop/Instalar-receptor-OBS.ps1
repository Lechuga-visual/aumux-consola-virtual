param([string]$ObsRoot = '')
$ErrorActionPreference = 'Stop'
if (Get-Process obs64 -ErrorAction SilentlyContinue) {
    throw 'Cierra OBS antes de instalar el receptor.'
}
$source = Join-Path $PSScriptRoot 'obs-plugin\monitor-mix-receiver.dll'
if (-not (Test-Path $source)) { throw "No se encuentra el receptor: $source. Extrae todo el ZIP." }
if ($ObsRoot) {
    $ObsRoot = (Resolve-Path $ObsRoot).Path
    if (-not (Test-Path (Join-Path $ObsRoot 'bin\64bit\obs64.exe'))) {
        throw 'ObsRoot debe ser la carpeta de OBS que contiene bin\64bit\obs64.exe.'
    }
    $destination = Join-Path $ObsRoot 'obs-plugins\64bit'
    $data = Join-Path $ObsRoot 'data\obs-plugins\monitor-mix-receiver'
} else {
    # Windows OBS uses ProgramData, NOT the user's Roaming AppData plugin folder.
    $base = Join-Path $env:ProgramData 'obs-studio\plugins\monitor-mix-receiver'
    $destination = Join-Path $base 'bin\64bit'
    $data = Join-Path $base 'data'
}
New-Item -ItemType Directory -Force -Path $destination, $data | Out-Null
$target = Join-Path $destination 'monitor-mix-receiver.dll'
if (Test-Path $target) {
    $backup = "$target.$(Get-Date -Format 'yyyyMMdd-HHmmss-fff').bak"
    Copy-Item $target $backup
    Write-Host "Copia previa conservada en $backup"
}
Copy-Item $source $target -Force
Unblock-File -Path $target
if ((Get-FileHash $source).Hash -ne (Get-FileHash $target).Hash) { throw 'La copia no coincide con el receptor original.' }
Write-Host "Receptor copiado y verificado en $target"
Write-Host 'Abre OBS x64 32.1 o posterior, sin modo seguro, y agrega la fuente Monitor Mix - master local.'
Write-Host 'Esto verifica los archivos; el registro de OBS confirma si el modulo se cargo.'
Write-Host 'Si no aparece, ejecuta Diagnosticar-OBS.cmd. Para OBS portable usa -ObsRoot con la carpeta de OBS.'
