# Run on Windows after unpacking the portable package. Requires a trusted code
# signing certificate in the Windows certificate store and Windows SDK signtool.
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$PackageDirectory,
    [Parameter(Mandatory=$true)][string]$CertificateThumbprint,
    [string]$TimestampServer = 'https://timestamp.digicert.com',
    [string]$SignTool = 'signtool.exe'
)
$ErrorActionPreference = 'Stop'
$PackageDirectory = (Resolve-Path -LiteralPath $PackageDirectory).Path
if (-not (Test-Path -LiteralPath (Join-Path $PackageDirectory 'MonitorMix.exe'))) { throw 'Select an extracted Monitor Mix release directory.' }
$certificate = Get-Item -LiteralPath "Cert:\CurrentUser\My\$CertificateThumbprint" -ErrorAction Stop
if (-not $certificate.HasPrivateKey) { throw 'The certificate has no accessible signing key.' }
if ($certificate.NotAfter -le (Get-Date)) { throw 'The signing certificate has expired.' }
$files = @('MonitorMix.exe', 'obs-plugin\monitor-mix-receiver.dll')
foreach ($relative in $files) {
    $file = Join-Path $PackageDirectory $relative
    & $SignTool sign /sha1 $CertificateThumbprint /fd SHA256 /tr $TimestampServer /td SHA256 $file
    if ($LASTEXITCODE -ne 0) { throw "Signing failed: $relative" }
    & $SignTool verify /pa /all /v $file
    if ($LASTEXITCODE -ne 0) { throw "Signature verification failed: $relative" }
}
foreach ($name in @('Instalar-receptor-OBS.ps1', 'Diagnosticar-OBS.ps1')) {
    $result = Set-AuthenticodeSignature -LiteralPath (Join-Path $PackageDirectory $name) -Certificate $certificate -HashAlgorithm SHA256 -TimestampServer $TimestampServer
    if ($result.Status -ne 'Valid') { throw "Script signature verification failed: $name ($($result.Status))" }
}
# Signing changes bytes: regenerate internal checksums and the release ZIP only
# after every signature has passed verification.
$lines = Get-ChildItem -LiteralPath $PackageDirectory -Recurse -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | Sort-Object FullName | ForEach-Object {
    $relative = $_.FullName.Substring($PackageDirectory.Length + 1).Replace('\', '/')
    "{0}  {1}" -f (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant(), $relative
}
$encoding = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText((Join-Path $PackageDirectory 'SHA256SUMS.txt'), ($lines -join "`n") + "`n", $encoding)
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = $PackageDirectory.TrimEnd('\', '/') + '-signed.zip'
if (Test-Path -LiteralPath $archive) { throw "Archive already exists: $archive. Move it before packaging another signed release." }
[System.IO.Compression.ZipFile]::CreateFromDirectory($PackageDirectory, $archive, [System.IO.Compression.CompressionLevel]::Optimal, $false)
$digest = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()
[System.IO.File]::WriteAllText($archive + '.sha256', "$digest  $([System.IO.Path]::GetFileName($archive))`n", $encoding)
Write-Host "Signed and verified package: $archive"
