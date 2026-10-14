# Tries dist\kabuk-setup.exe in the Hyper-V test VM (tools/vm/new-vm.ps1), a Windows with
# nothing else installed (no Visual C++ runtime): installs it silently, checks what it
# made (the programs, the Start menu and desktop shortcuts, Path, the uninstall entry),
# runs `kabuk --help` from Path, starts Kabuk and asks it its state. With -Uninstall it
# removes Kabuk instead and checks that everything went.
#
#   powershell -ExecutionPolicy Bypass -File tools/vm/try-setup.ps1 [-Uninstall]
#
# tools/vm/screen.ps1 shows the VM's screen afterwards.
param([switch]$Uninstall)

$ErrorActionPreference = 'Stop'
$Name = 'kabuk-test'
$PipeName = 'kabuk-vm-com1'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$setup = Join-Path $root 'dist\kabuk-setup.exe'
if (-not $Uninstall -and -not (Test-Path $setup)) { throw "no ${setup}: run tools/package.ps1 first" }

function Connect-Agent([int]$timeoutSeconds) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        try {
            $pipe = New-Object System.IO.Pipes.NamedPipeClientStream('.', $PipeName, [System.IO.Pipes.PipeDirection]::InOut)
            $pipe.Connect(5000)
            $reader = New-Object System.IO.StreamReader($pipe, [System.Text.Encoding]::UTF8)
            $writer = New-Object System.IO.StreamWriter($pipe, (New-Object System.Text.UTF8Encoding($false)))
            $writer.NewLine = "`n"
            $writer.AutoFlush = $true
            $writer.WriteLine('PING')
            $task = $reader.ReadLineAsync()
            if ($task.Wait(5000) -and $task.Result -eq 'PONG') { return @{ pipe = $pipe; reader = $reader; writer = $writer } }
            $pipe.Dispose()
        } catch {}
        Start-Sleep -Seconds 5
    }
    throw "the VM's agent did not answer within $timeoutSeconds s"
}

$vm = Get-VM -Name $Name
if ($vm.State -ne 'Running') { Start-VM -Name $Name; Write-Host "VM started" }
$agent = Connect-Agent 600
Write-Host "agent ready"

$job = 'setup-' + (Get-Date -Format 'yyyyMMdd-HHmmss')
$stage = Join-Path $env:TEMP "kabuk-vm-$job"
New-Item -ItemType Directory -Force $stage | Out-Null
if (-not $Uninstall) { Copy-Item $setup $stage }

$check = @'
$dir = Join-Path $env:LOCALAPPDATA 'Programs\Kabuk'
$desktop = [Environment]::GetFolderPath('Desktop')
$startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Kabuk'
$path = (Get-ItemProperty HKCU:\Environment -Name Path -ErrorAction SilentlyContinue).Path
$entry = Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Kabuk'
Write-Output ("programs: " + (@(Get-ChildItem $dir -Filter *.exe -ErrorAction SilentlyContinue | ForEach-Object Name) -join ', '))
Write-Output ("desktop shortcut: " + (Test-Path (Join-Path $desktop 'Kabuk.lnk')))
Write-Output ("start menu: " + (@(Get-ChildItem $startMenu -ErrorAction SilentlyContinue | ForEach-Object Name) -join ', '))
Write-Output ("in Path: " + ([bool]($path -and $path.ToLower().Contains($dir.ToLower()))))
Write-Output ("uninstall entry: " + $entry)
'@

if ($Uninstall) {
    $body = @"
`$ErrorActionPreference = 'Continue'
`$kabuk = Join-Path `$env:LOCALAPPDATA 'Programs\Kabuk\kabuk.exe'
& `$kabuk --uninstall --quiet 2>&1 | ForEach-Object { Write-Output `$_ }
Write-Output "uninstall exit code: `$LASTEXITCODE"
Start-Sleep -Seconds 4
Write-Output ("kabuk running: " + [bool](Get-Process kabuk -ErrorAction SilentlyContinue))
$check
exit 0
"@
} else {
    $body = @"
`$ErrorActionPreference = 'Continue'
`$setup = Join-Path `$PSScriptRoot 'kabuk-setup.exe'
`$p = Start-Process `$setup -ArgumentList '/S' -Wait -PassThru
Write-Output "setup exit code: `$(`$p.ExitCode)"
$check
# A new shell's Path, as a new terminal would have it.
`$env:Path = [Environment]::GetEnvironmentVariable('Path', 'User') + ';' + [Environment]::GetEnvironmentVariable('Path', 'Machine')
`$help = & kabuk --help 2>&1 | Select-Object -First 3
Write-Output ("kabuk --help from Path: " + (`$help -join ' | '))
Start-Process (Join-Path `$env:LOCALAPPDATA 'Programs\Kabuk\kabuk-watchdog.exe')
Start-Sleep -Seconds 10
Write-Output ("kabuk running: " + [bool](Get-Process kabuk -ErrorAction SilentlyContinue))
`$state = & kabuk msg query state 2>&1
Write-Output ("state: " + ((`$state | Out-String) -replace '\s+', ' '))
exit 0
"@
}
Set-Content -Encoding UTF8 -Path (Join-Path $stage 'job.ps1') -Value $body
$zip = Join-Path $env:TEMP "$job.zip"
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -Force
Copy-VMFile -Name $Name -SourcePath $zip -DestinationPath "C:\kabuk\jobs\$job.zip" -FileSource Host -CreateFullPath -Force
Remove-Item -Recurse -Force $stage, $zip

$agent.writer.WriteLine("RUN $job")
$code = 1
while ($true) {
    $line = $agent.reader.ReadLine()
    if ($null -eq $line) { Write-Host "the agent went away"; break }
    if ($line.StartsWith('OUT ')) { Write-Host $line.Substring(4); continue }
    if ($line -match '^DONE (-?\d+)$') { $code = [int]$Matches[1]; break }
}
$agent.pipe.Dispose()
exit $code
