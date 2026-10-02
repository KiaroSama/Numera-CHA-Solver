#requires -Version 7.0
[CmdletBinding()]
param([switch]$Yes, [ValidateSet('gpu','cpu')][string]$Engine='gpu')
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'setup'
$code=0
try {
    $root=Get-ProjectRoot
    $uv=Get-Command uv -ErrorAction SilentlyContinue
    if (-not $uv) { throw 'uv is required. Install it to your approved tools directory, then run setup again.' }
    if (-not $Yes) {
        $answer=Read-Host "Install locked project dependencies for $Engine inference? [Y/n]"
        if ($answer -and $answer -notmatch '^(?i)y(es)?$') { Write-RunLog INFO setup 'Installation declined; no changes made.'; exit 0 }
    }
    $code=Invoke-BoundedCommand -Executable $uv.Source -Arguments @('sync','--locked','--extra',$Engine,'--extra','browser') -WallSeconds 600 -IdleSeconds 120
    if ($code -ne 0) { throw 'Locked environment setup failed.' }
    Write-RunLog INFO setup 'Project environment installed. Models are downloaded only with models fetch.'
} catch { $code=1; Write-RunLog ERROR setup $_.Exception.Message }
finally { Write-RunLog INFO cleanup "Setup finished exit_code=$code." }
exit $code
