# Tries Kabuk's MSIX package (tools/msix.ps1, DECISIONS D84) in the Hyper-V test VM:
# registers the package's layout as it is (developer mode, turned on in the VM only),
# starts Kabuk from the package and asks it its state, checks that what it writes reaches
# Windows (its state folder and the screen saver it sets, seen from outside the package),
# that `kabuk` works as a command, that the package refuses `--install`, sets its start
# at sign-in through its startup task and comes back with `kabuk --restore`, then removes
# the package. With -Keep it stays installed and running (to look at with
# tools/vm/screen.ps1).
#
#   powershell -ExecutionPolicy Bypass -File tools/vm/try-msix.ps1 [-Keep]
param([switch]$Keep)

$ErrorActionPreference = 'Stop'
$Name = 'kabuk-test'
$PipeName = 'kabuk-vm-com1'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$layout = Join-Path $root 'target\msix\layout'
if (-not (Test-Path (Join-Path $layout 'AppxManifest.xml'))) { throw "no package layout: run tools/msix.ps1 first" }

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

$job = 'msix-' + (Get-Date -Format 'yyyyMMdd-HHmmss')
$stage = Join-Path $env:TEMP "kabuk-vm-$job"
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item -Recurse $layout (Join-Path $stage 'layout')

$body = @"
`$ErrorActionPreference = 'Continue'
`$layout = Join-Path `$PSScriptRoot 'layout'
`$family = 'Milvasoft.Kabuk_tzmzhd3z3x8ea'
`$alias = Join-Path `$env:LOCALAPPDATA 'Microsoft\WindowsApps\kabuk.exe'
`$state = Join-Path `$env:LOCALAPPDATA 'kabuk'
# A Kabuk installed by its setup (tools/vm/try-setup.ps1) steps aside first.
`$installed = Join-Path `$env:LOCALAPPDATA 'Programs\Kabuk\kabuk.exe'
if (Test-Path `$installed) { & `$installed --restore 2>&1 | Out-Null; Write-Output 'the setup-installed Kabuk restored' }
# Developer mode: a package's layout registers as it is (the VM only).
reg add 'HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock' /v AllowDevelopmentWithoutDevLicense /t REG_DWORD /d 1 /f 2>&1 | ForEach-Object { Write-Output "developer mode: `$_" }
Get-AppxPackage Milvasoft.Kabuk | Remove-AppxPackage
`$before = (Get-ItemProperty 'HKCU:\Control Panel\Desktop' -Name 'SCRNSAVE.EXE' -ErrorAction SilentlyContinue).'SCRNSAVE.EXE'
Write-Output "screen saver before: `$before"
try {
    Add-AppxPackage -Register (Join-Path `$layout 'AppxManifest.xml') -ErrorAction Stop
    Write-Output 'registered'
} catch {
    Write-Output "not registered: `$(`$_.Exception.Message)"
    exit 1
}
`$p = Get-AppxPackage Milvasoft.Kabuk
Write-Output "package: `$(`$p.PackageFullName) family `$(`$p.PackageFamilyName)"
Write-Output ("alias: " + (Test-Path `$alias))
`$help = & `$alias --help 2>&1 | Select-Object -First 2
Write-Output ("kabuk --help through the alias: " + (`$help -join ' | '))
`$install = & `$alias --install --quiet 2>&1 | Out-String
Write-Output ("kabuk --install: exit `${LASTEXITCODE}: " + (`$install -replace '\s+', ' '))
Start-Process "shell:AppsFolder\`$family!Kabuk"
Start-Sleep -Seconds 12
Write-Output ("kabuk running: " + [bool](Get-Process kabuk -ErrorAction SilentlyContinue))
`$q = & `$alias msg query state 2>&1 | Out-String
Write-Output ("state: " + (`$q -replace '\s+', ' ').Substring(0, [Math]::Min(300, (`$q -replace '\s+', ' ').Length)))
Write-Output ("state file seen from outside the package: " + (Test-Path (Join-Path `$state 'state.json')))
Write-Output ("screen saver file seen from outside the package: " + (Test-Path (Join-Path `$state 'kabuk-saver.scr')))
`$during = (Get-ItemProperty 'HKCU:\Control Panel\Desktop' -Name 'SCRNSAVE.EXE' -ErrorAction SilentlyContinue).'SCRNSAVE.EXE'
Write-Output "screen saver while it runs: `$during"
`$off = & `$alias --startup off 2>&1 | Out-String
Write-Output ("startup off: exit `${LASTEXITCODE}: " + (`$off -replace '\s+', ' '))
`$on = & `$alias --startup on 2>&1 | Out-String
Write-Output ("startup on: exit `${LASTEXITCODE}: " + (`$on -replace '\s+', ' '))
& `$alias --restore 2>&1 | ForEach-Object { Write-Output "restore: `$_" }
Start-Sleep -Seconds 4
Write-Output ("kabuk running after restore: " + [bool](Get-Process kabuk -ErrorAction SilentlyContinue))
`$after = (Get-ItemProperty 'HKCU:\Control Panel\Desktop' -Name 'SCRNSAVE.EXE' -ErrorAction SilentlyContinue).'SCRNSAVE.EXE'
Write-Output "screen saver after restore: `$after (as before: `$(`$after -eq `$before))"
if ('$Keep' -eq 'True') {
    Start-Process "shell:AppsFolder\`$family!Kabuk"
    Write-Output 'kept installed and running'
    exit 0
}
Get-AppxPackage Milvasoft.Kabuk | Remove-AppxPackage
Write-Output ("package after removal: " + [bool](Get-AppxPackage Milvasoft.Kabuk))
Write-Output ("alias after removal: " + (Test-Path `$alias))
exit 0
"@
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
