#requires -Version 7.0
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'stop'
$code=0
try {
    $root=Get-ProjectRoot
    $state=Join-Path $root 'data\server.json'
    if (-not (Test-Path -LiteralPath $state)) { Write-RunLog INFO cleanup 'No owned server record exists.'; exit 0 }
    $record=Get-Content -LiteralPath $state -Raw -Encoding utf8 | ConvertFrom-Json
    $owner=Get-Process -Id $record.owner_pid -ErrorAction SilentlyContinue
    if ($owner -and $owner.StartTime.ToUniversalTime().ToString('o') -eq $record.owner_start -and $record.root -eq $root) {
        $children=@(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($owner.Id)" | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($root,[StringComparison]::OrdinalIgnoreCase) })
        foreach($child in $children) {
            $process=Get-Process -Id $child.ProcessId -ErrorAction SilentlyContinue
            if ($process -and $process.StartTime -ge $owner.StartTime) { $process.Kill($true); $process.WaitForExit(5000) | Out-Null; Write-RunLog INFO cleanup "Stopped owned child pid=$($child.ProcessId)." }
        }
        $owner.WaitForExit(5000) | Out-Null
    } elseif($owner) { throw 'Ownership identity does not match; no process was stopped.' }
    Remove-Item -LiteralPath $state
    Write-RunLog INFO cleanup 'Owned server stopped or already exited.'
} catch { $code=1; Write-RunLog ERROR cleanup $_.Exception.Message }
exit $code
