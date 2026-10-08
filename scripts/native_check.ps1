#requires -Version 7.0
[CmdletBinding()]
param([ValidateSet('oracle','compile','parity','sdk-parity','preprocess','build','smoke','gpu-smoke','http-parity','startup-credential','worker-cancel','worker-fixture','decode-owner','launcher-owner','cli-filter','task-active')][string]$Operation='compile')
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$work=Join-Path $root '.ci-work/rust-migration'
[IO.Directory]::CreateDirectory($work)|Out-Null
$temp=Join-Path $work 'tmp'
[IO.Directory]::CreateDirectory($temp)|Out-Null
$env:TEMP=$temp
$env:TMP=$temp
$env:NUMERA_ROOT=$root
$env:CARGO_BUILD_JOBS='2'
$env:RUST_TEST_THREADS='1'
$hooks='G:/Program Files/Portable/Scripts/Hook Maker/hooks'
. (Join-Path $hooks '_processlib.ps1')
. (Join-Path $hooks '_repositoryaccess.ps1')
$snapshot=Get-RepositoryAccessSnapshot $root
if($snapshot.State -ne 'available'){throw 'Repository state could not be verified.'}
if($Operation -eq 'oracle'){
    $executable=Join-Path $root '.venv/Scripts/python.exe'
    $arguments=@((Join-Path $PSScriptRoot 'migration_oracle.py'),'--output',(Join-Path $root 'tests/fixtures/parity'))
}else{
    $executable=(& rustup which cargo).Trim()
    if($LASTEXITCODE -ne 0 -or -not [IO.File]::Exists($executable)){throw 'The selected native Cargo executable could not be resolved.'}
    $env:RUSTC=(& rustup which rustc).Trim()
    $arguments=switch($Operation){
        'compile' {@('check','--all-targets');break}
        'build' {@('build','--bin','numera-cha');break}
        'smoke' {$executable=Join-Path $root 'target/debug/numera-cha.exe'; @('recognize','--text','12 * 3 = ?');break}
        'task-active' {@('test','--test','worker','active_task_shutdown_joins_consumer_and_reaps_inference','--','--exact','--test-threads=1');break}
        'cli-filter' {@('test','--test','cli','image_character_filter_is_not_a_second_exclusive_input','--','--exact','--test-threads=1');break}
        'launcher-owner' {$executable=(Get-Command pwsh -ErrorAction Stop).Source; @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $root 'tests/launcher_ownership.ps1'));break}
        'decode-owner' {@('test','--test','decode','cancelled_and_expired_decoders_hold_admission_until_drained','--','--exact','--test-threads=1');break}
        'worker-fixture' {@('test','--test','worker','fixture_startup_and_binary_frame_roundtrip_without_engine','--','--exact','--test-threads=1');break}
        'worker-cancel' {@('test','--test','worker','handshake_and_inference_cancellation_reap_before_successor','--','--exact','--test-threads=1');break}
        'startup-credential' {@('test','--test','startup','credential_publication_replaces_atomically_and_failure_preserves_previous','--','--exact','--test-threads=1');break}
        'http-parity' {@('test','--test','http_parity','http_matches_source_schema_security_and_large_input_contracts','--','--exact','--test-threads=1');break}
        'preprocess' {@('test','--test','preprocessing','tensor_matches_pinned_pillow_source_bytes','--','--exact','--test-threads=1');break}
        'sdk-parity' {@('test','--test','parity','sdk_matches_generated_source_results','--','--exact','--test-threads=1');break}
        'gpu-smoke' {$executable=Join-Path $root 'target/debug/numera-cha.exe'; $env:NUMERA_ORT_LIBRARY=Join-Path $root '.venv/Lib/site-packages/onnxruntime/capi/onnxruntime.dll'; @('recognize','--image',(Join-Path $root 'tests/fixtures/ocr/ascii-smoke.png'));break}
        default {@('test','--test','parity','arithmetic_matches_generated_source_oracle','--','--exact','--test-threads=1')}
    }
}
& 'G:/Program Files/Portable/Scripts/Hook Maker/scripts/Run-Tests-Guarded.ps1' -ProjectFingerprint $snapshot.Fingerprint -FilePath $executable -ArgumentsJson (ConvertTo-Json -InputObject $arguments -Compress) -WorkingDirectory $root -MaxWorkers 2 -TimeoutSeconds 180 -IdleTimeoutSeconds 45 -HeartbeatSeconds 2 -ResultPath (Join-Path $work "$Operation-result.json")
exit $LASTEXITCODE
