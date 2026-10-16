# Builds a release of Kabuk (docs/PLAN.md M14): every check first, then the release
# build, then a zip holding the four programs, the README and the licence, ready to be
# unpacked anywhere and installed with `kabuk.exe --install`.
#
#   powershell -ExecutionPolicy Bypass -File tools/release.ps1              # checks, build, zip
#   powershell -ExecutionPolicy Bypass -File tools/release.ps1 -SkipChecks  # build and zip only
#
# The zip lands in target\release-zip\kabuk-<version>.zip with its SHA-256 next to it.

param(
    [switch]$SkipChecks
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$programs = @('kabuk.exe', 'kabuk-watchdog.exe', 'kabuk-settings.exe', 'kabuk-saver.exe')
$extras = @('README.md', 'LICENSE')

if (-not $SkipChecks) {
    & powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'check.ps1')
    if ($LASTEXITCODE -ne 0) {
        Write-Host "The checks failed; no release was made." -ForegroundColor Red
        exit $LASTEXITCODE
    }
}

Write-Host "==> cargo build --release" -ForegroundColor Cyan
cargo build --release -p kabuk -p kabuk-watchdog -p kabuk-settings -p kabuk-saver
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# The version is the workspace's.
$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$out = Join-Path $root 'target\release-zip'
$stage = Join-Path $out "kabuk-$version"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null
foreach ($p in $programs) {
    Copy-Item (Join-Path $root "target\release\$p") $stage
}
foreach ($e in $extras) {
    Copy-Item (Join-Path $root $e) $stage
}
$zip = Join-Path $out "kabuk-$version.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
$hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
Set-Content -Encoding ascii -Path "$zip.sha256" -Value "$hash  kabuk-$version.zip"
$size = [math]::Round((Get-Item $zip).Length / 1MB, 1)
Write-Host ""
Write-Host "Release: $zip ($size MB)" -ForegroundColor Green
Write-Host "SHA-256: $hash"
