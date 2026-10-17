# Builds Kabuk's MSIX package for the Microsoft Store (DECISIONS D84, PLAN M17): the
# release programs, packaging\AppxManifest.xml with the workspace's version, the tile and
# logo images (tools\msix-assets.py) and the package's texts from locales\*.ftl, packed
# into dist\kabuk-<version>.msix for Partner Center. The layout it packs stays in
# target\msix\layout, which `Add-AppxPackage -Register` installs as it is (developer mode,
# the test VM only).
#
#   powershell -ExecutionPolicy Bypass -File tools/msix.ps1
#
# Needs the Windows SDK (makeappx.exe, makepri.exe) and Python with Pillow.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$programs = @('kabuk.exe', 'kabuk-watchdog.exe', 'kabuk-settings.exe', 'kabuk-saver.exe')
# The package's texts: the name in the manifest and the locales' key for it.
$texts = [ordered]@{ AppName = 'setup-title'; AppDescription = 'store-description'; SettingsName = 'setup-shortcut-settings' }
$languages = [ordered]@{ 'en-US' = 'en'; 'tr-TR' = 'tr' }

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
# The Store's packages are versioned major.minor.build.0.
$packageVersion = "$version.0"
Write-Host "Kabuk $packageVersion (MSIX)" -ForegroundColor Cyan

# The newest Windows SDK's tools.
$sdk = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin' -Directory |
    Where-Object { Test-Path (Join-Path $_.FullName 'x64\makeappx.exe') } |
    Sort-Object { [version]$_.Name } | Select-Object -Last 1
if (-not $sdk) { throw 'The Windows SDK (makeappx.exe) was not found.' }
$makeappx = Join-Path $sdk.FullName 'x64\makeappx.exe'
$makepri = Join-Path $sdk.FullName 'x64\makepri.exe'

cargo build --release -p kabuk -p kabuk-watchdog -p kabuk-settings -p kabuk-saver
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$work = Join-Path $root 'target\msix'
$layout = Join-Path $work 'layout'
if (Test-Path $layout) { Remove-Item -Recurse -Force $layout }
New-Item -ItemType Directory -Force $layout | Out-Null
foreach ($p in $programs) {
    Copy-Item (Join-Path $root "target\release\$p") $layout
}

python tools\msix-assets.py (Join-Path $layout 'Assets')
if ($LASTEXITCODE -ne 0) { throw 'The package images were not made.' }

$utf8 = New-Object System.Text.UTF8Encoding $false
$manifest = [IO.File]::ReadAllText((Join-Path $root 'packaging\AppxManifest.xml'), $utf8).Replace('{VERSION}', $packageVersion)
[IO.File]::WriteAllText((Join-Path $layout 'AppxManifest.xml'), $manifest, $utf8)

# One Resources.resw per language, from that language's .ftl.
foreach ($lang in $languages.Keys) {
    $ftl = @{}
    foreach ($line in [IO.File]::ReadAllLines((Join-Path $root "locales\$($languages[$lang]).ftl"), $utf8)) {
        if ($line -match '^([a-z0-9-]+) = (.*)$') { $ftl[$Matches[1]] = $Matches[2] }
    }
    $entries = foreach ($name in $texts.Keys) {
        $value = $ftl[$texts[$name]]
        if (-not $value) { throw "locales\$($languages[$lang]).ftl has no $($texts[$name])" }
        "  <data name=`"$name`" xml:space=`"preserve`"><value>$([Security.SecurityElement]::Escape($value))</value></data>"
    }
    $resw = "<?xml version=`"1.0`" encoding=`"utf-8`"?>`n<root>`n" + ($entries -join "`n") + "`n</root>`n"
    $dir = Join-Path $layout "Strings\$lang"
    New-Item -ItemType Directory -Force $dir | Out-Null
    [IO.File]::WriteAllText((Join-Path $dir 'Resources.resw'), $resw, $utf8)
}

$priconfig = Join-Path $work 'priconfig.xml'
& $makepri createconfig /cf $priconfig /dq ($languages.Keys -join '_') /pv 10.0.0 /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'makepri createconfig failed.' }
& $makepri new /pr $layout /cf $priconfig /mn (Join-Path $layout 'AppxManifest.xml') /of (Join-Path $layout 'resources.pri') /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'makepri new failed.' }

$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$msix = Join-Path $dist "kabuk-$version.msix"
& $makeappx pack /d $layout /p $msix /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'makeappx pack failed.' }

$size = [math]::Round((Get-Item $msix).Length / 1MB, 1)
Write-Host "dist\kabuk-$version.msix  $size MB" -ForegroundColor Green
