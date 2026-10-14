# The test VM's agent (installed by tools/vm/autounattend.xml, runs at logon in the
# VM's desktop session). It listens on COM1, which Hyper-V connects to a named pipe on
# the host (tools/vm/run.ps1), one command per line:
#
#   PING          -> PONG
#   RUN <name>    unpacks C:\kabuk\jobs\<name>.zip (copied in by the host with
#                 Copy-VMFile) and runs its job.ps1; each output line comes back as
#                 "OUT <line>", then "DONE <exit code>".
$ErrorActionPreference = 'Continue'
$jobs = 'C:\kabuk\jobs'
New-Item -ItemType Directory -Force $jobs | Out-Null
$log = 'C:\kabuk\agent.log'

function Say([System.IO.Ports.SerialPort]$port, [string]$line) {
    $port.WriteLine($line)
}

while ($true) {
    try {
        $port = New-Object System.IO.Ports.SerialPort 'COM1', 115200, 'None', 8, 'One'
        $port.Encoding = [System.Text.Encoding]::UTF8
        $port.NewLine = "`n"
        $port.ReadTimeout = [System.IO.Ports.SerialPort]::InfiniteTimeout
        $port.Open()
        Add-Content $log "$(Get-Date -Format o) COM1 open"
        while ($true) {
            $line = $port.ReadLine().Trim()
            if (-not $line) { continue }
            Add-Content $log "$(Get-Date -Format o) <- $line"
            if ($line -eq 'PING') { Say $port 'PONG'; continue }
            if ($line -match '^RUN ([A-Za-z0-9_-]+)$') {
                $name = $Matches[1]
                $zip = Join-Path $jobs "$name.zip"
                $dir = Join-Path $jobs $name
                if (-not (Test-Path $zip)) { Say $port "OUT no job file $zip"; Say $port 'DONE 98'; continue }
                Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
                Expand-Archive -Path $zip -DestinationPath $dir -Force
                $p = New-Object System.Diagnostics.Process
                $p.StartInfo.FileName = 'powershell.exe'
                $p.StartInfo.Arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$dir\job.ps1`""
                $p.StartInfo.WorkingDirectory = $dir
                $p.StartInfo.UseShellExecute = $false
                $p.StartInfo.RedirectStandardOutput = $true
                $p.StartInfo.RedirectStandardError = $true
                $p.StartInfo.StandardOutputEncoding = [System.Text.Encoding]::UTF8
                $p.Start() | Out-Null
                $err = $p.StandardError.ReadToEndAsync()
                while (-not $p.StandardOutput.EndOfStream) { Say $port ("OUT " + $p.StandardOutput.ReadLine()) }
                $p.WaitForExit()
                foreach ($e in ($err.Result -split "`r?`n")) { if ($e) { Say $port "OUT $e" } }
                Say $port "DONE $($p.ExitCode)"
                continue
            }
            Say $port "OUT unknown command: $line"
            Say $port 'DONE 99'
        }
    } catch {
        Add-Content $log "$(Get-Date -Format o) $($_.Exception.Message)"
        try { $port.Close() } catch {}
        Start-Sleep -Seconds 2
    }
}
