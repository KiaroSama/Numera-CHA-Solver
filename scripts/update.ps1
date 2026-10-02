#requires -Version 7.0
[CmdletBinding()]
param([switch]$Yes, [ValidateSet('gpu','cpu')][string]$Engine='gpu')
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'update'
$code=0
try {
    if (-not $Yes) {
        $answer=Read-Host 'Synchronize the committed lockfile without changing model revisions? [Y/n]'
        if ($answer -and $answer -notmatch '^(?i)y(es)?$') { Write-RunLog INFO update 'Update declined; no changes made.'; exit 0 }
    }
    $uv=(Get-Command uv -ErrorAction Stop).Source
    $code=Invoke-BoundedCommand -Executable $uv -Arguments @('sync','--locked','--extra',$Engine,'--extra','browser') -WallSeconds 600 -IdleSeconds 120
    if ($code -ne 0) { throw 'Locked update failed; model revisions and previous model assets were not changed.' }
    Write-RunLog INFO update 'Locked dependencies synchronized; existing model revisions preserved.'
} catch { $code=1; Write-RunLog ERROR update $_.Exception.Message }
finally { Write-RunLog INFO cleanup "Update finished exit_code=$code." }
exit $code
