#requires -Version 7.0
[CmdletBinding()]
param([switch]$Gpu, [switch]$Browser)
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'ci'
$code=0
try {
    $root=Get-ProjectRoot
    $python=Get-ProjectPython
    $uv=(Get-Command uv -ErrorAction Stop).Source
    $commands=@(
        @{exe=$python; args=@('-m','ruff','format','--check','src','tests','scripts')},
        @{exe=$python; args=@('-m','ruff','check','src','tests','scripts')},
        @{exe=(Join-Path $root '.venv\Scripts\ty.exe'); args=@('check','src')},
        @{exe=$python; args=@('scripts/check_artifacts.py','--source')}
    )
    foreach($command in $commands) {
        [Console]::Out.WriteLine('CI stage: static verification')
        $code=Invoke-BoundedCommand -Executable $command.exe -Arguments $command.args -WallSeconds 60 -IdleSeconds 30
        if ($code -ne 0) { throw 'Static verification failed.' }
    }
    $env:NUMERA_TEST_GPU=if($Gpu){'1'}else{'0'}
    $env:NUMERA_TEST_BROWSER=if($Browser){'1'}else{'0'}
    $env:NUMERA_MAX_TEST_WORKERS='1'
    [Console]::Out.WriteLine('CI stage: deterministic, device and browser tests')
    $testArgs=@('-m','pytest','--timeout=15','--durations=10','--junitxml=.ci-work/results.xml','--cov=numera_cha_solver','--cov-report=term-missing','tests')
    $code=Invoke-BoundedCommand -Executable $python -Arguments $testArgs -WallSeconds 300 -IdleSeconds 120
    if ($code -ne 0) { throw 'Deterministic test verification failed.' }
    $code=Invoke-BoundedCommand -Executable $uv -Arguments @('build','--out-dir','.ci-work\packages') -WallSeconds 120 -IdleSeconds 45
    if ($code -ne 0) { throw 'Package build failed.' }
    $code=Invoke-BoundedCommand -Executable $python -Arguments @('scripts/check_artifacts.py','--packages','.ci-work/packages') -WallSeconds 60 -IdleSeconds 30
    if ($code -ne 0) { throw 'Package content verification failed.' }
    Write-RunLog INFO verification 'All requested deterministic CI gates passed.'
} catch { $code=1; Write-RunLog ERROR verification $_.Exception.Message }
finally { Write-RunLog INFO cleanup "CI finished exit_code=$code." }
exit $code
