#Requires -Version 5.1
<#
.SYNOPSIS
    OledSaver'ı bu kullanıcının ekran koruyucusu olarak kurar. Yönetici yetkisi gerekmez.

.PARAMETER TimeoutMinutes
    Hareketsizlikten sonra kaç dakika beklensin. Varsayılan 5.
#>
param(
    [ValidateRange(1, 600)]
    [int]$TimeoutMinutes = 5
)

$ErrorActionPreference = 'Stop'

$source = Join-Path $PSScriptRoot 'OledSaver.scr'
$targetDir = Join-Path $env:LOCALAPPDATA 'OledSaver'
$target = Join-Path $targetDir 'OledSaver.scr'
$desktopKey = 'HKCU:\Control Panel\Desktop'
$policyKey = 'HKCU:\Software\Policies\Microsoft\Windows\Control Panel\Desktop'
$timeoutSeconds = $TimeoutMinutes * 60

if (-not (Test-Path $source)) {
    throw "OledSaver.scr bulunamadı: $source"
}

# Running instance would lock the file during an upgrade.
Get-CimInstance Win32_Process -Filter "Name = 'OledSaver.scr'" -ErrorAction SilentlyContinue |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }

New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
Copy-Item -Path $source -Destination $target -Force
Unblock-File -Path $target

# Windows has historically been picky about spaces in SCRNSAVE.EXE; prefer the 8.3 path when available.
$saverPath = $target
try {
    $shortPath = (New-Object -ComObject Scripting.FileSystemObject).GetFile($target).ShortPath
    if ($shortPath) { $saverPath = $shortPath }
}
catch {
    # 8.3 names can be disabled on the volume; the long path works on current Windows.
}

Set-ItemProperty -Path $desktopKey -Name 'SCRNSAVE.EXE' -Value $saverPath

Add-Type -Namespace OledSaverSetup -Name Spi -MemberDefinition @'
[DllImport("user32.dll", SetLastError = true)]
public static extern bool SystemParametersInfo(uint uiAction, uint uiParam, System.IntPtr pvParam, uint fWinIni);
'@

$SPI_SETSCREENSAVETIMEOUT = 0x000F
$SPI_SETSCREENSAVEACTIVE = 0x0011
$SPIF_UPDATEINIFILE_SENDCHANGE = 0x0003

# Applies immediately (no sign-out needed) and persists to the registry.
[void][OledSaverSetup.Spi]::SystemParametersInfo($SPI_SETSCREENSAVETIMEOUT, [uint32]$timeoutSeconds, [IntPtr]::Zero, $SPIF_UPDATEINIFILE_SENDCHANGE)
[void][OledSaverSetup.Spi]::SystemParametersInfo($SPI_SETSCREENSAVEACTIVE, 1, [IntPtr]::Zero, $SPIF_UPDATEINIFILE_SENDCHANGE)

Write-Host ''
Write-Host "OledSaver kuruldu: $target" -ForegroundColor Green
Write-Host "Bekleme süresi: $TimeoutMinutes dk. Video oynarken Windows ekran koruyucuyu başlatmaz."

if (Test-Path $policyKey) {
    $policy = Get-ItemProperty -Path $policyKey
    if ($policy.'SCRNSAVE.EXE' -or $policy.ScreenSaveActive -or $policy.ScreenSaveTimeOut) {
        Write-Warning "Grup ilkesi ekran koruyucu ayarlarını zorluyor ($policyKey). Buradaki ayarlar geçersiz kalabilir."
    }
}

# If the display turns off before the saver starts, it would never be seen.
try {
    $hexValues = [regex]::Matches((powercfg /query SCHEME_CURRENT SUB_VIDEO VIDEOIDLE | Out-String), '0x[0-9a-fA-F]+')
    if ($hexValues.Count -ge 2) {
        $displayOffSeconds = [Convert]::ToInt32($hexValues[$hexValues.Count - 2].Value, 16)
        if ($displayOffSeconds -gt 0 -and $displayOffSeconds -le $timeoutSeconds) {
            Write-Warning ("Güç ayarlarında ekran {0} dk sonra kapanıyor; ekran koruyucu hiç görünmez. Ekran kapanmayı {1} dk'dan uzun yap (OLED için 15-20 dk iyi)." -f [math]::Round($displayOffSeconds / 60, 1), $TimeoutMinutes)
        }
    }
}
catch {
    # Informational only.
}

Write-Host ''
Write-Host 'Hemen denemek için:  ' -NoNewline
Write-Host "& '$target' /s" -ForegroundColor Cyan
Write-Host 'Ayarlar için:        ' -NoNewline
Write-Host "& '$target' /c" -ForegroundColor Cyan
