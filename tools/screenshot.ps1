# Captures the whole virtual screen (all monitors) in physical pixels to a PNG, and
# prints every monitor's bounds and work area. Development tool for manual checks.
#
#   powershell -ExecutionPolicy Bypass -File tools/screenshot.ps1 -Out shot.png [-Region x,y,w,h]
param(
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$Region = ""
)

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Dpi {
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
}
'@
# Per-monitor v2, so coordinates and the capture are in physical pixels.
[void][Dpi]::SetProcessDpiAwarenessContext([IntPtr]-4)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing

foreach ($s in [System.Windows.Forms.Screen]::AllScreens) {
    "{0} primary={1} bounds={2} work={3}" -f $s.DeviceName, $s.Primary, $s.Bounds, $s.WorkingArea
}

$v = [System.Windows.Forms.SystemInformation]::VirtualScreen
if ($Region) {
    $p = $Region.Split(',') | ForEach-Object { [int]$_ }
    $rect = New-Object System.Drawing.Rectangle($p[0], $p[1], $p[2], $p[3])
} else {
    $rect = $v
}
$bmp = New-Object System.Drawing.Bitmap($rect.Width, $rect.Height)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($rect.X, $rect.Y, 0, 0, $rect.Size)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
"saved $Out ($($rect.Width)x$($rect.Height) at $($rect.X),$($rect.Y))"
