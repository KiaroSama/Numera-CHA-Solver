#requires -Version 7.0
[CmdletBinding()]
param([switch]$Yes,[ValidateSet('gpu','cpu')][string]$Engine='gpu',[string]$RuntimeSourceRoot='')
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'setup'
$code=0
try{
    $root=Get-ProjectRoot
    if(-not $Yes){
        $answer=Read-Host "Build the locked native application for $Engine inference and stage approved local runtime libraries? [Y/n]"
        if($answer -and $answer -notmatch '^(?i)y(es)?$'){Write-RunLog INFO setup 'Installation declined; no application changes made.';exit 0}
    }
    if(-not [IO.File]::Exists((Join-Path $root 'Cargo.toml'))){
        $null=Get-NativeExecutable
        Write-RunLog INFO setup 'Native package is already installed; no build tool or Python application is required.'
    }else{
        Initialize-NativeEnvironment
        $code=Invoke-BoundedCommand -Executable (Get-RustTool cargo) -Arguments @('build','--locked','--release','--bin','numera-cha','--jobs','2') -WallSeconds 1200 -IdleSeconds 180
        if($code -ne 0){throw 'Locked native build failed; previous binary and model assets were not removed.'}
        $bin=Join-Path $root 'bin';[IO.Directory]::CreateDirectory($bin)|Out-Null
        $temporary=Join-Path $bin "numera-cha.$([guid]::NewGuid().ToString('N')).tmp"
        try{[IO.File]::Copy((Join-Path $root 'target/release/numera-cha.exe'),$temporary,$false);[IO.File]::Move($temporary,(Join-Path $bin 'numera-cha.exe'),$true)}finally{if([IO.File]::Exists($temporary)){[IO.File]::Delete($temporary)}}
    }
    if(-not $RuntimeSourceRoot){$RuntimeSourceRoot=$root}
    Copy-NativeRuntime -SourceRoot $RuntimeSourceRoot -Engine $Engine
    Write-RunLog INFO setup 'Native application ready. No global tools installed; models are acquired only with models fetch. CPU use remains explicit.'
}catch{if($code -eq 0){$code=1};Write-RunLog ERROR setup $_.Exception.Message}
finally{Write-RunLog INFO cleanup "Setup finished exit_code=$code."}
exit $code
