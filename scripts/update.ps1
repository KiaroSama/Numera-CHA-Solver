#requires -Version 7.0
[CmdletBinding()]
param([switch]$Yes,[ValidateSet('gpu','cpu')][string]$Engine='gpu',[string]$RuntimeSourceRoot='')
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'update'
$code=0
try{
    $root=Get-ProjectRoot
    if(-not [IO.File]::Exists((Join-Path $root 'Cargo.toml'))){throw 'This installed package contains no source. Replace it with an inspected native package; existing data and models must be preserved.'}
    if(-not $Yes){
        $answer=Read-Host 'Rebuild the committed native lockfile without changing model revisions? [Y/n]'
        if($answer -and $answer -notmatch '^(?i)y(es)?$'){Write-RunLog INFO update 'Update declined; no application changes made.';exit 0}
    }
    $arguments=@('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $PSScriptRoot 'setup.ps1'),'-Yes','-Engine',$Engine)
    if($RuntimeSourceRoot){$arguments+=@('-RuntimeSourceRoot',$RuntimeSourceRoot)}
    $code=Invoke-BoundedCommand -Executable (Get-Command pwsh -ErrorAction Stop).Source -Arguments $arguments -WallSeconds 1300 -IdleSeconds 240
    if($code -ne 0){throw 'Locked native update failed; model revisions and user data were not removed.'}
    Write-RunLog INFO update 'Locked native application rebuilt; model revisions preserved.'
}catch{if($code -eq 0){$code=1};Write-RunLog ERROR update $_.Exception.Message}
finally{Write-RunLog INFO cleanup "Update finished exit_code=$code."}
exit $code
