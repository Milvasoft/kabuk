# Builds Kabuk's setup: the release programs, then kabuk-setup.exe holding them, into
# dist\ with its SHA-256 (for the GitHub release).
#
#   powershell -ExecutionPolicy Bypass -File tools/package.ps1
#
# Upload dist\kabuk-setup.exe to the GitHub release named v<version>.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
Write-Host "Kabuk $version" -ForegroundColor Cyan

cargo build --release --workspace
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$env:KABUK_SETUP_PAYLOAD = Join-Path $root 'target\release'
cargo build --release -p kabuk-setup
$built = $LASTEXITCODE
Remove-Item Env:\KABUK_SETUP_PAYLOAD
if ($built -ne 0) { exit $built }

$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$setup = Join-Path $dist 'kabuk-setup.exe'
Copy-Item (Join-Path $root 'target\release\kabuk-setup.exe') $setup -Force
$hash = (Get-FileHash $setup -Algorithm SHA256).Hash
Set-Content -Path (Join-Path $dist 'kabuk-setup.exe.sha256') -Value "$hash  kabuk-setup.exe" -Encoding ascii

$size = [math]::Round((Get-Item $setup).Length / 1MB, 1)
Write-Host "dist\kabuk-setup.exe  $size MB  SHA256 $hash" -ForegroundColor Green
