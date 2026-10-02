#requires -Version 7.0
[CmdletBinding()]
param([string]$Smoke='')
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'doctor'
$code=0
try {
    $arguments=@('-m','numera_cha_solver','doctor')
    if ($Smoke) { $arguments+=@('--smoke',$Smoke) }
    $code=Invoke-BoundedCommand -Executable (Get-ProjectPython) -Arguments $arguments -WallSeconds 130 -IdleSeconds 45
} catch { $code=1; Write-RunLog ERROR doctor $_.Exception.Message }
finally { Write-RunLog INFO cleanup "Doctor finished exit_code=$code." }
exit $code
