# Types into the test VM's console as a keyboard would (Hyper-V's virtual keyboard),
# for looking into the VM when its agent does not answer.
#
#   powershell -ExecutionPolicy Bypass -File tools/vm/type.ps1 -Run "<command line>"   # Win+R, the line, Enter
#   powershell -ExecutionPolicy Bypass -File tools/vm/type.ps1 -Text "<text>" [-Enter]
param(
    [string]$Run,
    [string]$Text,
    [switch]$Enter
)
$ErrorActionPreference = 'Stop'
$Name = 'kabuk-test'
$vm = Get-CimInstance -Namespace root\virtualization\v2 -ClassName Msvm_ComputerSystem -Filter "ElementName='$Name'"
$keyboard = Get-CimAssociatedInstance -InputObject $vm -ResultClassName Msvm_Keyboard
function Key([string]$method, [int]$code) {
    Invoke-CimMethod -InputObject $keyboard -MethodName $method -Arguments @{ keyCode = [uint32]$code } | Out-Null
}
# Not `Type`: PowerShell's `type` alias (Get-Content) would win over the function.
function Send-Text([string]$s) {
    Invoke-CimMethod -InputObject $keyboard -MethodName TypeText -Arguments @{ asciiText = $s } | Out-Null
}
$VK_LWIN = 0x5B; $VK_R = 0x52; $VK_RETURN = 0x0D
if ($Run) {
    Key PressKey $VK_LWIN; Key TypeKey $VK_R; Key ReleaseKey $VK_LWIN
    Start-Sleep -Milliseconds 800
    Send-Text $Run
    Key TypeKey $VK_RETURN
}
if ($Text) {
    Send-Text $Text
    if ($Enter) { Key TypeKey $VK_RETURN }
}
