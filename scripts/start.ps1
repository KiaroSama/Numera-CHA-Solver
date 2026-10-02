#requires -Version 7.0
[CmdletBinding()]
param([ValidateRange(1,65535)][int]$Port=8765)
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'start'
$code=0
try {
    $root=Get-ProjectRoot
    $state=Join-Path $root 'data\server.json'
    if (Test-Path -LiteralPath $state) { throw 'A server ownership record exists. Run stop.ps1 or inspect that record before starting another instance.' }
    [IO.Directory]::CreateDirectory((Join-Path $root 'data')) | Out-Null
    $python=Get-ProjectPython
    Write-RunLog INFO startup "Starting authenticated loopback dashboard on port=$Port."
    $record=@{ owner_pid=$PID; owner_start=(Get-Process -Id $PID).StartTime.ToUniversalTime().ToString('o'); root=$root }
    [IO.File]::WriteAllText($state,($record|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
    try {
        $code=Invoke-BoundedCommand -Executable $python -Arguments @('-m','numera_cha_solver','serve','--port',"$Port") -WallSeconds 86400 -IdleSeconds 86400
    } finally { Remove-Item -LiteralPath $state -ErrorAction SilentlyContinue }
} catch { $code=1; Write-RunLog ERROR startup $_.Exception.Message }
finally { Write-RunLog INFO cleanup "Server launcher finished exit_code=$code." }
exit $code
