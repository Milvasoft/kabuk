# Saves what the test VM's screen shows as a PNG, without connecting to it (Hyper-V's
# thumbnail image), to see how an unattended install or a test is going.
#
#   powershell -ExecutionPolicy Bypass -File tools/vm/screen.ps1 <out.png> [-Width 1024] [-Height 768]
param(
    [Parameter(Mandatory = $true)][string]$Out,
    [int]$Width = 1024,
    [int]$Height = 768
)
$ErrorActionPreference = 'Stop'
$Name = 'kabuk-test'
Add-Type -AssemblyName System.Drawing
$ns = 'root\virtualization\v2'
$service = Get-CimInstance -Namespace $ns -ClassName Msvm_VirtualSystemManagementService
$vm = Get-CimInstance -Namespace $ns -ClassName Msvm_ComputerSystem -Filter "ElementName='$Name'"
$settings = Get-CimAssociatedInstance -InputObject $vm -ResultClassName Msvm_VirtualSystemSettingData |
    Where-Object { $_.VirtualSystemType -eq 'Microsoft:Hyper-V:System:Realized' }
$r = Invoke-CimMethod -InputObject $service -MethodName GetVirtualSystemThumbnailImage -Arguments @{
    TargetSystem = $settings; WidthPixels = [uint16]$Width; HeightPixels = [uint16]$Height
}
if ($r.ReturnValue -ne 0) { throw "no image (code $($r.ReturnValue))" }
# The image is 16-bit RGB 5-6-5, top row first.
$bytes = [byte[]]$r.ImageData
$bmp = New-Object System.Drawing.Bitmap($Width, $Height, [System.Drawing.Imaging.PixelFormat]::Format16bppRgb565)
$rect = New-Object System.Drawing.Rectangle(0, 0, $Width, $Height)
$data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::WriteOnly, $bmp.PixelFormat)
for ($y = 0; $y -lt $Height; $y++) {
    [System.Runtime.InteropServices.Marshal]::Copy($bytes, $y * $Width * 2, [IntPtr]($data.Scan0.ToInt64() + $y * $data.Stride), $Width * 2)
}
$bmp.UnlockBits($data)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
Write-Host "saved $Out"
