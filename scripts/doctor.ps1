#requires -Version 7.0
[CmdletBinding()]
param([string]$Smoke='',[ValidateSet('cuda','cpu')][string]$Device='cuda')
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'doctor'
$code=0
try{
    $env:NUMERA_ROOT=Get-ProjectRoot
    $arguments=@('--device',$Device,'doctor')
    if($Smoke){$arguments+=@('--smoke',[IO.Path]::GetFullPath($Smoke))}
    $code=Invoke-BoundedCommand -Executable (Get-NativeExecutable) -Arguments $arguments -WallSeconds 130 -IdleSeconds 45
}catch{if($code -eq 0){$code=1};Write-RunLog ERROR doctor $_.Exception.Message}
finally{Write-RunLog INFO cleanup "Doctor finished exit_code=$code."}
exit $code
