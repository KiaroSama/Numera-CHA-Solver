#requires -Version 7.0
[CmdletBinding()]
param([ValidateSet('Generate','Rollback')][string]$Operation='Generate',[Parameter(Mandatory)][string]$Work,[string]$Executable='')
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'source-oracle'
$code=0;$snapshot=$null;$previousEnvironment=$env:UV_PROJECT_ENVIRONMENT
try{
    $root=Get-ProjectRoot;$Work=[IO.Path]::GetFullPath($Work)
    if(-not $Work.StartsWith($root.TrimEnd('\','/')+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Oracle work must remain inside the project.'}
    [IO.Directory]::CreateDirectory($Work)|Out-Null
    $snapshot=Join-Path $Work ('source-'+[guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($snapshot)|Out-Null
    $pin='a8e25292317621e900d908c56927c489eecae06a'
    $git=(Get-Command git -ErrorAction Stop).Source
    $archive=Join-Path $snapshot 'source.zip'
    $code=Invoke-BoundedCommand $git @('archive','--format=zip',"--output=$archive",$pin,'src/numera_cha_solver','tests','pyproject.toml','uv.lock','.python-version','LICENSE','THIRD_PARTY_NOTICES.md') -WallSeconds 30 -IdleSeconds 15
    if($code -ne 0){throw 'Pinned Git source extraction failed.'}
    Expand-Archive -LiteralPath $archive -DestinationPath $snapshot
    [IO.File]::Delete($archive)
    if(-not $env:NUMERA_ORACLE_PYTHON){
        $uv=(Get-Command uv -ErrorAction Stop).Source
        $environment=Join-Path $root '.ci-oracle-env'
        $env:UV_PROJECT_ENVIRONMENT=$environment
        $code=Invoke-BoundedCommand $uv @('sync','--frozen','--python','3.14.8','--extra','gpu','--extra','browser') -WallSeconds 120 -IdleSeconds 45 -WorkingDirectory $snapshot
        if($code -ne 0){throw 'Locked test-only source-oracle environment could not be restored.'}
    }
    $python=Get-ProjectPython
    $env:NUMERA_ORACLE_SOURCE=$snapshot
    if($Operation -eq 'Generate'){
        $code=Invoke-BoundedCommand $python @((Join-Path $PSScriptRoot 'migration_oracle.py'),'--output',$Work) -WallSeconds 90 -IdleSeconds 30
    }else{
        if(-not $Executable){throw 'Native rollback executable is required.'}
        $suffix=if($IsWindows){'.exe'}else{''}
        $controller=Join-Path $root "target/debug/examples/hosted_rollback$suffix"
        if(-not [IO.File]::Exists($controller)){throw 'Hosted native rollback controller is missing; compile CI examples first.'}
        $code=Invoke-BoundedCommand $controller @('--python',$python,'--executable',$Executable,'--work',$Work) -WallSeconds 90 -IdleSeconds 30
    }
    if($code -ne 0){throw 'Pinned source verification failed.'}
}catch{if($code -eq 0){$code=1};Write-RunLog ERROR oracle $_.Exception.Message}
finally{
    $env:NUMERA_ORACLE_SOURCE=$null
    $env:UV_PROJECT_ENVIRONMENT=$previousEnvironment
    if($snapshot -and [IO.Directory]::Exists($snapshot)){[IO.Directory]::Delete($snapshot,$true)}
    Write-RunLog INFO cleanup "Pinned source temporary snapshot removed exit_code=$code."
}
exit $code
