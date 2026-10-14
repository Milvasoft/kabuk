#Requires -Version 5.1
<#
.SYNOPSIS
    OledSaver'ı kaldırır ve ekran koruyucuyu kapatır.
#>

$ErrorActionPreference = 'Stop'

$targetDir = Join-Path $env:LOCALAPPDATA 'OledSaver'
$desktopKey = 'HKCU:\Control Panel\Desktop'
$settingsKey = 'HKCU:\Software\OledSaver'

Get-CimInstance Win32_Process -Filter "Name = 'OledSaver.scr'" -ErrorAction SilentlyContinue |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }

$current = (Get-ItemProperty -Path $desktopKey -Name 'SCRNSAVE.EXE' -ErrorAction SilentlyContinue).'SCRNSAVE.EXE'

# Only touch the active screen saver if it is ours (short or long path both contain the folder name).
if ($current -and ($current -like '*OledSaver*' -or $current -like '*OLEDSA~*')) {
    Remove-ItemProperty -Path $desktopKey -Name 'SCRNSAVE.EXE'

    Add-Type -Namespace OledSaverSetup -Name Spi -MemberDefinition @'
[DllImport("user32.dll", SetLastError = true)]
public static extern bool SystemParametersInfo(uint uiAction, uint uiParam, System.IntPtr pvParam, uint fWinIni);
'@

    $SPI_SETSCREENSAVEACTIVE = 0x0011
    $SPIF_UPDATEINIFILE_SENDCHANGE = 0x0003

    [void][OledSaverSetup.Spi]::SystemParametersInfo($SPI_SETSCREENSAVEACTIVE, 0, [IntPtr]::Zero, $SPIF_UPDATEINIFILE_SENDCHANGE)
}

if (Test-Path $targetDir) {
    Remove-Item -Path $targetDir -Recurse -Force
}

if (Test-Path $settingsKey) {
    Remove-Item -Path $settingsKey -Recurse -Force
}

Write-Host 'OledSaver kaldırıldı.' -ForegroundColor Green
