#requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][ValidateSet('linux-x64','macos-arm64')][string]$Platform)
Import-Module (Join-Path $PSScriptRoot '../desktop/ci/Hosted.psm1') -Force
Initialize-HostedLog 'native-diagnosis'
$root=Get-HostedRoot
$work=Join-Path $root '.ci-work/diagnosis'
[IO.Directory]::CreateDirectory($work)|Out-Null
$policy=Join-Path $work 'owner-policy'
Assert-Exit (Invoke-Hosted rustc @('--edition=2024','--test','tests/native_owner.rs','-o',$policy) 60 30) 'compile-owner-policy'
Assert-Exit (Invoke-Hosted $policy @('--test-threads=1') 15 5) 'owner-policy'
Assert-Exit (Invoke-Hosted $env:NUMERA_CI_OWNER_BINARY @('--contract','--work',(Join-Path $work 'owner-contract')) 45 15) 'owner-contract'
if($Platform -eq 'macos-arm64'){
    Assert-Exit (Invoke-Hosted cargo @('build','--locked','--release','-p','numera-cha-solver','--bins','--jobs','2') 1200 240) 'actual-release-compile-ownership'
}
if($Platform -in @('linux-x64','macos-arm64')){
    Assert-Exit (Invoke-Hosted cargo @('build','--locked','-p','numera-cha-solver','--bins','--example','dashboard_browser','--jobs','2') 600 180) 'browser-consumer-build'
    $watch=[Diagnostics.Stopwatch]::StartNew()
    Assert-Exit (Invoke-Hosted (Join-Path $root 'target/debug/examples/dashboard_browser') @('--executable',(Join-Path $root 'target/debug/numera-cha'),'--driver',$env:NUMERA_BROWSER_DRIVER,'--browser',$env:NUMERA_BROWSER_EXECUTABLE,'--work',(Join-Path $root '.ci-work/diagnosis/browser')) 120 45) 'cold-browser-real-scenario'
    Write-HostedLog INFO diagnosis "Cold browser and complete dashboard elapsed_seconds=$($watch.Elapsed.TotalSeconds)."
}
Write-HostedLog INFO diagnosis 'Focused owner/browser diagnosis passed; this is not release eligibility.'
