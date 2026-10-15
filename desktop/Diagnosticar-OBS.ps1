param([string]$ObsRoot = '')
$ErrorActionPreference = 'Stop'
Write-Host 'Monitor Mix - diagnostico de OBS (solo lectura)'
Write-Host "Usuario: $env:USERNAME"
$roots = @((Join-Path $env:ProgramData 'obs-studio\plugins\monitor-mix-receiver'), (Join-Path $env:APPDATA 'obs-studio\plugins\monitor-mix-receiver'))
if ($ObsRoot) { $roots += (Join-Path $ObsRoot 'obs-plugins\64bit') }
foreach ($root in $roots) {
    Write-Host "`nRuta: $root"
    if (Test-Path $root) {
        Get-ChildItem $root -Recurse -Filter '*.dll' | ForEach-Object {
            Write-Host $_.FullName
            Write-Host (Get-FileHash $_.FullName).Hash
        }
    } else { Write-Host 'No existe.' }
}
$process = Get-Process obs64 -ErrorAction SilentlyContinue
if ($process) { $process | Select-Object Id, Path | Format-List | Out-Host }
$logs = Join-Path $env:APPDATA 'obs-studio\logs'
if ($ObsRoot) { $logs = Join-Path $ObsRoot 'config\obs-studio\logs' }
$latest = Get-ChildItem $logs -Filter '*.txt' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($latest) {
    Write-Host "`nRegistro mas reciente: $($latest.FullName)"
    Select-String -Path $latest.FullName -Pattern 'OBS Studio|monitor.mix|Failed to load|not loaded|safe mode|portable mode|incompatible' -Context 1, 2 | ForEach-Object { Write-Host $_ }
} else { Write-Host 'No se encontro registro. Abre OBS una vez o indica -ObsRoot para portable.' }
Write-Host "`nUDP 17840 (solo existe cuando se crea la fuente):"
Get-NetUDPEndpoint -LocalPort 17840 -ErrorAction SilentlyContinue | Format-Table LocalAddress, LocalPort, OwningProcess | Out-Host
Write-Host 'Si el modulo se carga pero no suena: crea/reutiliza su fuente en la escena activa, activa MASTER a OBS y el motor/generador.'
