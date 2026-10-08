#requires -Version 7.0
[CmdletBinding()]
param([ValidateRange(1,65535)][int]$Port=8765,[ValidateSet('cuda','cpu')][string]$Device='cuda')
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'start'
$code=0
try{
    $root=Get-ProjectRoot;$env:NUMERA_ROOT=$root
    $state=Join-Path $root 'data/server.json'
    if([IO.File]::Exists($state)){throw 'A server ownership record exists. Run stop.ps1 or inspect the record before another start.'}
    [IO.Directory]::CreateDirectory((Join-Path $root 'data'))|Out-Null
    $native=Get-NativeExecutable
    $env:NUMERA_SERVER_PORT="$Port"
    Write-RunLog INFO startup "Starting authenticated native loopback dashboard port=$Port device=$Device."
    $code=Invoke-BoundedCommand -Executable $native -Arguments @('--device',$Device,'serve','--port',"$Port") -WallSeconds 86400 -IdleSeconds 86400 -OwnershipPath $state
}catch{if($code -eq 0){$code=1};Write-RunLog ERROR startup $_.Exception.Message}
finally{Write-RunLog INFO cleanup "Server launcher finished exit_code=$code."}
exit $code
