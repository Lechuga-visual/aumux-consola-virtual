$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
Write-Host 'Requiere Rust 1.95 y Visual Studio C++ Build Tools con el SDK de Windows.'
& cargo +1.95.0 build --release --locked --target x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { throw 'Fallo la compilacion.' }
& .\target\x86_64-pc-windows-msvc\release\aumux.exe --self-test
if ($LASTEXITCODE -ne 0) { throw 'Fallo la prueba sin hardware.' }
Write-Host 'Este script compila AuMux. Para el receptor OBS consulta tools/build-windows-cloud.sh.'
