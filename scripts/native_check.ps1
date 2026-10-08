#requires -Version 7.0
[CmdletBinding()]
param([ValidateSet('oracle','compile','parity','sdk-parity','preprocess','build','smoke','gpu-smoke','http-parity','startup-credential','worker-cancel','worker-fixture','decode-owner','launcher-owner','cli-filter','task-active')][string]$Operation='compile')
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'native-check'
$code=0
try{
    $root=Get-ProjectRoot
    Initialize-NativeEnvironment
    if($Operation -in @('worker-cancel','worker-fixture','task-active')){
        $fixture=Join-Path $root '.ci-work/worker-fixture.exe'
        [IO.Directory]::CreateDirectory((Split-Path -Parent $fixture))|Out-Null
        $env:NUMERA_FIXTURE_WORKER_EXECUTABLE=$fixture
        $code=Invoke-BoundedCommand (Get-RustTool rustc) @((Join-Path $root 'tests/fixtures/worker.rs'),'--edition=2024','--crate-type=bin','-o',$fixture) -WallSeconds 60 -IdleSeconds 30
        if($code -ne 0){throw 'Native controlled worker fixture did not compile.'}
    }
    $executable=Get-RustTool cargo
    $arguments=switch($Operation){
        'compile' {@('check','--locked','--all-targets');break}
        'build' {@('build','--locked','--bin','numera-cha');break}
        'oracle' {$executable=(Get-Command pwsh -ErrorAction Stop).Source; @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $PSScriptRoot 'source_oracle.ps1'),'-Work',(Join-Path $root '.ci-work/oracle-development'));break}
        'smoke' {$executable=Get-NativeExecutable; @('recognize','--text','12 * 3 = ?');break}
        'gpu-smoke' {$executable=Get-NativeExecutable; @('doctor','--smoke',(Join-Path $root 'tests/fixtures/ocr/ascii-smoke.png'));break}
        'launcher-owner' {$executable=(Get-Command pwsh -ErrorAction Stop).Source; @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $root 'tests/launcher_ownership.ps1'));break}
        default {
            $target,$name=switch($Operation){
                'parity' {@('parity','arithmetic_matches_generated_source_oracle')}
                'sdk-parity' {@('parity','sdk_matches_generated_source_results')}
                'preprocess' {@('preprocessing','tensor_matches_pinned_pillow_source_bytes')}
                'http-parity' {@('http_parity','http_matches_source_schema_security_and_large_input_contracts')}
                'startup-credential' {@('startup','credential_publication_replaces_atomically_and_failure_preserves_previous')}
                'worker-cancel' {@('worker','handshake_and_inference_cancellation_reap_before_successor')}
                'worker-fixture' {@('worker','fixture_startup_and_binary_frame_roundtrip_without_engine')}
                'decode-owner' {@('decode','cancelled_and_expired_decoders_hold_admission_until_drained')}
                'cli-filter' {@('cli','image_character_filter_is_not_a_second_exclusive_input')}
                'task-active' {@('worker','active_task_shutdown_joins_consumer_and_reaps_inference')}
            }
            @('test','--locked','--test',$target,$name,'--','--exact','--test-threads=1')
        }
    }
    $env:RUST_TEST_THREADS='1'
    $code=Invoke-BoundedCommand -Executable $executable -Arguments $arguments -WallSeconds 180 -IdleSeconds 45
}catch{if($code -eq 0){$code=1};Write-RunLog ERROR verification $_.Exception.Message}
finally{Write-RunLog INFO cleanup "Focused native check finished exit_code=$code."}
exit $code
