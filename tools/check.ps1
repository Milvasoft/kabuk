# Runs every check a change must pass before it is called done (docs/TESTING.md §6):
# formatting, clippy with warnings as errors, unit/property/snapshot tests, the
# Windows integration tests, and the design page's contrast audit.
#
#   powershell -ExecutionPolicy Bypass -File tools/check.ps1            # everything
#   powershell -ExecutionPolicy Bypass -File tools/check.ps1 -SkipDesign  # no Node/Playwright step (CI)
#
# Exits with a non-zero code on the first failing step.

param(
    [switch]$SkipDesign
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Step([string]$name, [scriptblock]$command) {
    Write-Host ""
    Write-Host "==> $name" -ForegroundColor Cyan
    $started = Get-Date
    & $command
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FAILED: $name (exit $LASTEXITCODE)" -ForegroundColor Red
        exit $LASTEXITCODE
    }
    $seconds = [math]::Round(((Get-Date) - $started).TotalSeconds, 1)
    Write-Host "ok: $name ($seconds s)" -ForegroundColor Green
}

Step 'cargo fmt --check' { cargo fmt --all -- --check }
Step 'cargo clippy (all targets, all features, -D warnings)' { cargo clippy --workspace --all-targets --all-features -- -D warnings }
# The integration tests start kabuk-watchdog.exe and kabuk-testapp.exe from the target directory.
Step 'cargo build --workspace' { cargo build --workspace }
# Every test file runs even when one fails, so one failure cannot hide another.
Step 'cargo test --workspace' { cargo test --workspace --no-fail-fast }
# The integration tests run the real shell on a desktop. A CI runner has none they can
# use (no taskbar of Explorer's to hide, no windows to capture): there they run in the
# test VM instead (tools/vm/run.ps1).
if ($env:CI) {
    Write-Host ""
    Write-Host "==> integration tests: not on a CI runner (no desktop); run tools/vm/run.ps1" -ForegroundColor Yellow
} else {
    Step 'integration tests' { cargo test -p kabuk --features integration --no-fail-fast -- --test-threads=1 }
}

if (-not $SkipDesign) {
    if (-not (Test-Path (Join-Path $root 'node_modules/playwright'))) {
        Write-Host "Design tooling missing. Run once: npm install; npx playwright install chromium" -ForegroundColor Red
        exit 1
    }
    Step 'theme tokens match the design' { node tools/extract-themes.js --check }
    Step 'icons match the design' { node tools/extract-icons.js --check }
    Step 'translations match the design' { node tools/extract-locales.js --check }
    Step 'settings files match the design' { node tools/extract-settings.js --check }
    Step 'settings page against the bridge' {
        cargo build -p kabuk-settings --example bridge_stdio
        if ($LASTEXITCODE -eq 0) { node tools/settings-test.js }
    }
    Step 'design contrast audit' { node tools/contrast-audit.js docs/design/kabuk-tasarim.html }
}

Write-Host ""
Write-Host "All checks passed." -ForegroundColor Green
