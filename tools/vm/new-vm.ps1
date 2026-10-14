# Creates the Hyper-V test VM in which tools/vm/run.ps1 runs the Windows integration
# tests, so they never touch the desktop the user works on (docs/TESTING.md).
#
#   powershell -ExecutionPolicy Bypass -File tools/vm/new-vm.ps1 [-Root E:\kabuk-vm] [-Iso <windows 11 iso>]
#
# Needs: Hyper-V on, and the user in the "Hyper-V Administrators" group (no elevation).
# The VM: generation 2, no network (nothing updates during tests), COM1 on the host pipe
# \\.\pipe\kabuk-vm-com1 for the agent, guest services on for Copy-VMFile. Windows is
# installed unattended from the ISO with tools/vm/autounattend.xml; the guest's random
# password is kept in <Root>\guest-password.txt (never in the repository).
param(
    [string]$Root = 'E:\kabuk-vm',
    [string]$Iso = 'E:\kabuk-vm\win11-eval.iso',
    [int]$MemoryGB = 8,
    [int]$Cpus = 4,
    [int]$DiskGB = 80
)

$ErrorActionPreference = 'Stop'
$Name = 'kabuk-test'
$Pipe = '\\.\pipe\kabuk-vm-com1'
$here = $PSScriptRoot

if (Get-VM -Name $Name -ErrorAction SilentlyContinue) { throw "VM '$Name' already exists" }
if (-not (Test-Path $Iso)) { throw "ISO not found: $Iso" }
New-Item -ItemType Directory -Force $Root | Out-Null

# The guest account's password: random, kept next to the VM.
$pwFile = Join-Path $Root 'guest-password.txt'
if (-not (Test-Path $pwFile)) {
    $chars = [char[]]'abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789'
    $bytes = New-Object byte[] 24
    [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
    Set-Content -NoNewline -Path $pwFile -Value (-join ($bytes | ForEach-Object { $chars[$_ % $chars.Length] }))
}
$password = Get-Content -Raw $pwFile

# The answer disc: autounattend.xml with the password, and the agent.
$answer = Join-Path $Root 'answer'
Remove-Item -Recurse -Force $answer -ErrorAction SilentlyContinue
New-Item -ItemType Directory $answer | Out-Null
(Get-Content -Raw (Join-Path $here 'autounattend.xml')).Replace('@@PASSWORD@@', $password) |
    Set-Content -Encoding UTF8 (Join-Path $answer 'autounattend.xml')
Copy-Item (Join-Path $here 'agent.ps1') $answer

Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices.ComTypes;
public static class KabukIso {
    public static void Write(string path, object image, int blockSize, int blocks) {
        var stream = (IStream)image;
        var buffer = new byte[blockSize];
        using (var file = File.Create(path)) {
            for (int i = 0; i < blocks; i++) {
                stream.Read(buffer, blockSize, IntPtr.Zero);
                file.Write(buffer, 0, blockSize);
            }
        }
    }
}
'@
$fsi = New-Object -ComObject IMAPI2FS.MsftFileSystemImage
$fsi.FileSystemsToCreate = 3   # ISO 9660 and Joliet
$fsi.VolumeName = 'KABUK_ANSWER'
$fsi.Root.AddTree($answer, $false)
$image = $fsi.CreateResultImage()
$answerIso = Join-Path $Root 'answer.iso'
[KabukIso]::Write($answerIso, $image.ImageStream, $image.BlockSize, $image.TotalBlocks)

# The VM.
$vhd = Join-Path $Root "$Name.vhdx"
New-VM -Name $Name -Generation 2 -MemoryStartupBytes ($MemoryGB * 1GB) -NewVHDPath $vhd -NewVHDSizeBytes ($DiskGB * 1GB) -Path $Root | Out-Null
Get-VMNetworkAdapter -VMName $Name | Remove-VMNetworkAdapter
Set-VMProcessor -VMName $Name -Count $Cpus
Set-VMMemory -VMName $Name -DynamicMemoryEnabled $false
Set-VM -Name $Name -AutomaticCheckpointsEnabled $false -AutomaticStartAction Nothing -AutomaticStopAction ShutDown
Add-VMDvdDrive -VMName $Name -Path $Iso
Add-VMDvdDrive -VMName $Name -Path $answerIso
$setupDvd = Get-VMDvdDrive -VMName $Name | Where-Object Path -eq $Iso
Set-VMFirmware -VMName $Name -FirstBootDevice $setupDvd -EnableSecureBoot On -SecureBootTemplate MicrosoftWindows
Set-VMComPort -VMName $Name -Number 1 -Path $Pipe
Enable-VMIntegrationService -VMName $Name -Name 'Guest Service Interface'

# Start it and answer the firmware's "Press any key to boot from CD or DVD".
Start-VM -Name $Name
$vm = Get-CimInstance -Namespace root\virtualization\v2 -ClassName Msvm_ComputerSystem -Filter "ElementName='$Name'"
$keyboard = Get-CimAssociatedInstance -InputObject $vm -ResultClassName Msvm_Keyboard
$enter = 13
for ($i = 0; $i -lt 30; $i++) {
    Invoke-CimMethod -InputObject $keyboard -MethodName TypeKey -Arguments @{ keyCode = [uint32]$enter } | Out-Null
    Start-Sleep -Milliseconds 500
}
Write-Host "VM '$Name' is installing Windows unattended; tools/vm/run.ps1 -Wait waits for its agent."
