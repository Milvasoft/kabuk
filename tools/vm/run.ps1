# Runs Kabuk's Windows integration tests in the Hyper-V test VM (tools/vm/new-vm.ps1),
# so they need not wait for the user's desktop to be free and never disturb it.
#
#   powershell -ExecutionPolicy Bypass -File tools/vm/run.ps1 [-Filter <test name part>] [-Test bar,tray] [-Wait]
#
# Builds on the host, copies the programs and test binaries into the VM (Copy-VMFile) at
# the same paths (the tests find kabuk.exe by its build path), and has the VM's agent
# run them in its desktop session over COM1 (\\.\pipe\kabuk-vm-com1), printing their
# output here. -Wait first waits (up to an hour) for a VM that is still installing.
# Exits with the tests' exit code.
param(
    [string]$Filter = '',
    [string[]]$Test = @(),
    [switch]$Wait
)

$ErrorActionPreference = 'Stop'
# `-File` hands `-Test wm,ipc` over as one string.
$Test = @($Test | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
$Name = 'kabuk-test'
$PipeName = 'kabuk-vm-com1'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location $root

# Once the VM has restarted, the COM pipe can carry nothing until Hyper-V makes it again
# (seen after the VM's own restarts): it is made again while the agent stays silent.
$PipeRenewSeconds = 60

function Connect-Agent([int]$timeoutSeconds) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    $renew = (Get-Date).AddSeconds($PipeRenewSeconds)
    while ((Get-Date) -lt $deadline) {
        if ((Get-Date) -gt $renew -and (Get-VM -Name $Name).State -eq 'Running') {
            Set-VMComPort -VMName $Name -Number 1 -Path "\\.\pipe\$PipeName" -DebuggerMode Off
            $renew = (Get-Date).AddSeconds($PipeRenewSeconds)
        }
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
$agent = Connect-Agent $(if ($Wait) { 3600 } else { 300 })
Write-Host "agent ready"

# Build on the host.
cargo build --workspace
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
# cargo's "Compiling" lines on stderr would stop the script under 'Stop' (a design of the
# gallery changed meanwhile builds kabuk-wallgen again).
$ErrorActionPreference = 'Continue'
$artifacts = cargo test -p kabuk --features integration --no-run --message-format=json 2>$null |
    ForEach-Object { try { $_ | ConvertFrom-Json } catch {} } |
    Where-Object { $_.reason -eq 'compiler-artifact' -and $_.executable -and $_.target.kind -contains 'test' }
$built = $LASTEXITCODE
$ErrorActionPreference = 'Stop'
if ($built -ne 0) { Write-Host "test build failed"; exit $built }
$tests = $artifacts | Where-Object { $Test.Count -eq 0 -or $Test -contains $_.target.name } | ForEach-Object { $_.executable }

# Stage the job: programs and tests under their repository paths, and the script to run.
$job = 'job-' + (Get-Date -Format 'yyyyMMdd-HHmmss')
$stage = Join-Path $env:TEMP "kabuk-vm-$job"
$payload = Join-Path $stage 'payload'
$debug = Join-Path $root 'target\debug'
$files = @('kabuk.exe', 'kabuk-watchdog.exe', 'kabuk-saver.exe', 'kabuk-testapp.exe', 'kabuk-settings.exe', 'WebView2Loader.dll') |
    ForEach-Object { Join-Path $debug $_ }
$files += $tests
$runtime = Join-Path $env:SystemRoot 'System32\vcruntime140.dll'
foreach ($f in $files) {
    $rel = $f.Substring($root.Length).TrimStart('\')
    $dest = Join-Path $payload $rel
    New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null
    Copy-Item $f $dest
    # The Visual C++ runtime (redistributable) next to each program, as the VM has none.
    Copy-Item $runtime (Split-Path $dest) -Force
}
$testLines = ($tests | ForEach-Object { "    '" + $_.Substring($root.Length).TrimStart('\') + "'" }) -join ",`n"
@"
`$ErrorActionPreference = 'Continue'
`$root = '$root'
# A checkout new to the VM (a worktree): without its folder, Copy-Item would make the
# payload's one folder (target) the checkout itself.
New-Item -ItemType Directory -Force `$root | Out-Null
Copy-Item -Recurse -Force (Join-Path `$PSScriptRoot 'payload\*') `$root
# Nobody moves the VM's mouse: it rests in the middle, not over a bar the tests hide.
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
`$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
[System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point((`$screen.Width / 2), (`$screen.Height / 2))
`$tests = @(
$testLines
)
`$failed = 0
foreach (`$t in `$tests) {
    Write-Output "==> `$t"
    & (Join-Path `$root `$t) --test-threads=1 $Filter 2>&1 | ForEach-Object { Write-Output `$_ }
    if (`$LASTEXITCODE -ne 0) { `$failed = `$LASTEXITCODE }
}
exit `$failed
"@ | Set-Content -Encoding UTF8 (Join-Path $stage 'job.ps1')
$zip = Join-Path $env:TEMP "$job.zip"
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -Force
Copy-VMFile -Name $Name -SourcePath $zip -DestinationPath "C:\kabuk\jobs\$job.zip" -FileSource Host -CreateFullPath -Force
Remove-Item -Recurse -Force $stage, $zip

# Run it and print what it says.
$agent.writer.WriteLine("RUN $job")
$code = 1
while ($true) {
    $line = $agent.reader.ReadLine()
    if ($null -eq $line) { Write-Host "the agent went away"; break }
    if ($line.StartsWith('OUT ')) { Write-Host $line.Substring(4); continue }
    if ($line -match '^DONE (-?\d+)$') { $code = [int]$Matches[1]; break }
}
$agent.pipe.Dispose()
Write-Host "VM tests finished with exit code $code"
exit $code
