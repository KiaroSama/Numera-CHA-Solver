#requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][ValidateSet('windows-x64','linux-x64','macos-arm64')][string]$Platform,[Parameter(Mandatory)][string]$Target)
Import-Module (Join-Path $PSScriptRoot '../desktop/ci/Hosted.psm1') -Force
Initialize-HostedLog 'hosted-ci'
$code=0;$results=[Collections.Generic.List[object]]::new();$failures=[Collections.Generic.List[string]]::new()
$root=Get-HostedRoot;$work=Join-Path $root '.ci-work/hosted';[IO.Directory]::CreateDirectory($work)|Out-Null
function Stage([string]$Name,[string]$Executable,[string[]]$Arguments,[int]$Wall=120,[int]$Idle=45,[string]$Capture='',[string]$Cwd=$root) {
    Write-HostedLog INFO verification "Stage=$Name started wall=$Wall idle=$Idle."
    $watch=[Diagnostics.Stopwatch]::StartNew();$status=1
    try { $status=Invoke-Hosted $Executable $Arguments $Wall $Idle $Capture $Cwd }
    catch { Write-HostedLog ERROR verification $_.Exception.Message }
    $results.Add(@{name=$Name;exit_code=$status;duration_seconds=$watch.Elapsed.TotalSeconds})
    if ($status -ne 0) { $failures.Add($Name) }
    Write-HostedLog $(if($status -eq 0){'INFO'}else{'ERROR'}) verification "Stage=$Name exit_code=$status."
    return $status
}
function Required([string]$Name,[string]$Executable,[string[]]$Arguments,[int]$Wall=120,[int]$Idle=45,[string]$Capture='',[string]$Cwd=$root) {
    Assert-Exit (Stage $Name $Executable $Arguments $Wall $Idle $Capture $Cwd) $Name
}
function Test-InstalledLaunchers([string]$Installed) {
    # Reuse the maintained Windows launcher ownership regression and actual installed entry points.
    Required 'installed-doctor-launcher' pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $Installed 'scripts/doctor.ps1'),'-Device','cpu') 130 45
    Required 'installed-setup-no-source' pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $Installed 'scripts/setup.ps1'),'-Yes','-Engine','cpu') 60 30
    Required 'installed-stop-without-owner' pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $Installed 'scripts/stop.ps1')) 30 15
    $script=Join-Path $root 'tests/installed_launchers.ps1'
    Required 'installed-start-stop-and-credential-redaction' pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',$script,'-Installed',$Installed,'-Work',$work,'-Common',(Join-Path $PSScriptRoot 'Common.psm1')) 90 30
}
try {
    $env:CARGO_BUILD_JOBS='2';$env:RUST_TEST_THREADS='1';$env:NUMERA_MAX_TEST_WORKERS='1'
    $env:NUMERA_TEST_GPU='0';$env:NUMERA_TEST_BROWSER='1';$env:NUMERA_DEVICE='cpu';$env:NUMERA_ROOT=$root
    if (-not $env:NUMERA_RUNTIME_ROOT -or -not $env:NUMERA_ORT_LIBRARY) { throw 'Verified CPU runtime/model fixture is required.' }
    $suffix=if($IsWindows){'.exe'}else{''};$pwsh=Get-HostedCommand pwsh
    if (-not $env:NUMERA_BROWSER_EXECUTABLE -or -not $env:NUMERA_BROWSER_DRIVER) { throw 'Native headless browser/driver paths are required.' }
    $expected=@('parity','preprocessing','adapters','lifecycle','gpu','cli','http_parity','core_contracts','startup','worker','task_regressions','downloads','decode','mcp','mcp_bridge','review','platform','client_payload','native_webdriver','fixture_inventory','native_owner','distribution')
    $actual=@(Get-ChildItem -LiteralPath (Join-Path $root 'tests') -File -Filter '*.rs'|ForEach-Object {$_.BaseName})
    if (@(Compare-Object ($expected|Sort-Object) ($actual|Sort-Object)).Count) { throw 'Native test coverage inventory differs; map the new tier explicitly instead of omitting it.' }
    $utf8=[Text.UTF8Encoding]::new($false,$true)
    foreach ($relative in (& git -C $root ls-files)) {
        $path=Join-Path $root $relative
        if ([IO.Path]::GetExtension($relative) -in @('.rs','.cs','.py','.ps1','.psm1','.md','.json','.toml','.lock','.yml','.yaml','.html','.css','.js') -or [IO.Path]::GetFileName($relative) -in @('.gitignore','LICENSE')) {
            $text=$utf8.GetString([IO.File]::ReadAllBytes($path))
            if ([IO.Path]::GetExtension($relative) -in @('.rs','.py','.ps1','.psm1','.cs','.js') -and ($text -split "`n").Count -gt 800) { throw 'Source file exceeds its line ceiling.' }
        }
    }
    Required 'hosted-command-and-notice-contracts' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $root 'desktop/ci/hosted_contracts.ps1')) 30 15
    if (-not $IsWindows) {
        Required 'posix-owned-process-regression' $env:NUMERA_CI_OWNER_BINARY @('--contract','--work',(Join-Path $work 'owner-contract')) 45 15
    }
    Required 'format' cargo @('fmt','--all','--','--check') 60 30
    Required 'locked-workspace-dependencies' cargo @('fetch','--locked') 300 120
    Required 'native-debug-build' cargo @('build','--locked','-p','numera-cha-solver','--bins','--jobs','2') 600 180
    Required 'native-ci-consumers-build' cargo @('build','--locked','-p','numera-cha-solver','--examples','--jobs','2') 600 180
    $fixture=Join-Path $work "worker-fixture$suffix"
    Required 'synthetic-native-worker-once' rustc @((Join-Path $root 'tests/fixtures/worker.rs'),'--edition=2024','--crate-type=bin','-o',$fixture) 60 30
    $env:NUMERA_FIXTURE_WORKER_EXECUTABLE=$fixture;$env:NUMERA_WORKER_EXECUTABLE=$null
    Required 'native-release-build' cargo @('build','--locked','--release','-p','numera-cha-solver','--bins','--jobs','2') 1200 240
    if ($IsWindows -or $IsMacOS) {
        foreach($name in @('numera-cha','numera-cha-solver-mcp')) {
            Required "sign-native-$name" $pwsh @('-NoProfile','-File',(Join-Path $PSScriptRoot 'sign_release.ps1'),'-Phase','Native','-Path',(Join-Path $root "target/release/$name$suffix")) 45 30
        }
    }
    Required 'matching-source-and-desktop-resources' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $PSScriptRoot 'package_hosted.ps1'),'-Phase','Resources','-Platform',$Platform,'-Target',$Target) 180 60
    Required 'desktop-icon-from-approved-logo' cargo @('tauri','icon',(Join-Path $root 'resources/web/logo.png'),'--output',(Join-Path $root 'desktop/icons')) 60 30 '' (Join-Path $root 'desktop')
    $null=Stage 'clippy-all-native-and-desktop' cargo @('clippy','--locked','--workspace','--all-targets','--jobs','2','--','-D','warnings') 1200 180
    $testCapture=Join-Path $work 'native-tests.txt'
    $tests=Stage 'all-native-and-desktop-tests' cargo @('test','--locked','--workspace','--all-targets','--no-fail-fast','--jobs','2','--','--test-threads=1') 600 120 $testCapture
    if (-not [IO.File]::Exists($testCapture)) { throw 'Native suite result capture is missing; validation cannot be claimed.' }
    if ([IO.File]::Exists($testCapture)) {
        $output=[IO.File]::ReadAllText($testCapture,[Text.Encoding]::UTF8)
        if ($tests -eq 0 -and ($output -match '[1-9][0-9]* ignored;' -or $output -notmatch 'test result: ok\.')) { throw 'Native suite skipped a required case or provided no test results.' }
        foreach ($summary in [regex]::Matches($output,'(?m)^test result:.*$')) {
            Write-HostedLog $(if($tests -eq 0){'INFO'}else{'ERROR'}) native-tests $summary.Value
        }
        if ($tests -ne 0) {
            foreach ($section in [regex]::Matches($output,'(?ms)^failures:.*?(?=^test result:|\z)')) {
                Write-HostedLog ERROR native-tests $section.Value.Substring(0,[Math]::Min($section.Value.Length,8192))
            }
        }
    }
    $null=Stage 'native-headless-browser' (Join-Path $root "target/debug/examples/dashboard_browser$suffix") @('--executable',(Join-Path $root "target/debug/numera-cha$suffix"),'--driver',$env:NUMERA_BROWSER_DRIVER,'--browser',$env:NUMERA_BROWSER_EXECUTABLE,'--work',(Join-Path $work 'browser')) 120 45
    if ($IsWindows) {
        $null=Stage 'windows-owned-launcher-tree' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $root 'tests/launcher_ownership.ps1')) 90 30
    }
    if($IsMacOS){$publisher=[IO.File]::ReadAllText((Join-Path $root 'resources/signing/publisher.json'),[Text.Encoding]::UTF8)|ConvertFrom-Json;$env:APPLE_SIGNING_IDENTITY=$publisher.thumbprint}
    $bundle=if($IsWindows){'nsis'}elseif($IsMacOS){'app'}else{'deb'}
    Required 'native-desktop-build-and-bundle' cargo @('tauri','build','--ci','--bundles',$bundle,'--','--locked','--jobs','2') 1200 240 '' (Join-Path $root 'desktop')
    if ($Platform -eq 'linux-x64') {
        if ($failures.Count) { throw 'Native validation failures block desktop execution and artifact creation.' }
        $deb=@(Get-ChildItem -LiteralPath (Join-Path $root 'target/release/bundle/deb') -File -Filter '*.deb')
        if ($deb.Count -ne 1) { throw 'Linux native installer discovery failed.' }
        Required 'linux-install-inspected-desktop' sudo @('-n','dpkg','-i',$deb[0].FullName) 60 30
        $nativeClose=Join-Path $work 'close-native-window'
        Required 'linux-native-close-fixture' rustc @((Join-Path $root 'desktop/ci/close_native_window.rs'),'--edition=2024','--crate-type=bin','-l','X11','-o',$nativeClose) 30 15
        Required 'linux-packaged-native-webdriver' xvfb-run @('-a',(Join-Path $root 'target/debug/examples/native_webdriver'),'--executable','/usr/bin/numera-cha-desktop','--driver',(Join-Path $root '.ci-work/tools/bin/tauri-driver'),'--native-close',$nativeClose,'--work',(Join-Path $work 'desktop-webdriver')) 120 30
    }
    Required 'final-native-distributions' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $PSScriptRoot 'package_hosted.ps1'),'-Phase','Archives','-Platform',$Platform,'-Target',$Target) 180 60
    Required 'signer-tamper-and-wrong-key-contracts' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $root 'tests/signing_contracts.ps1'),'-Artifacts',(Join-Path $root '.ci-work/artifacts')) 60 30
    $installed=Join-Path $work 'installed native sample'
    [IO.Compression.ZipFile]::ExtractToDirectory((Join-Path $root ".ci-work/artifacts/numera-cha-0.4.0-$Platform.zip"),$installed)
    if (-not $IsWindows) { foreach ($file in (Get-ChildItem -LiteralPath (Join-Path $installed 'bin') -File)) { [IO.File]::SetUnixFileMode($file.FullName,[IO.UnixFileMode]::UserRead -bor [IO.UnixFileMode]::UserWrite -bor [IO.UnixFileMode]::UserExecute -bor [IO.UnixFileMode]::GroupRead -bor [IO.UnixFileMode]::GroupExecute -bor [IO.UnixFileMode]::OtherRead -bor [IO.UnixFileMode]::OtherExecute) } }
    $null=Stage 'installed-cli-connector-and-real-cpu-ocr' (Join-Path $root "target/debug/examples/installed_release$suffix") @('--root',$installed,'--runtime-root',$env:NUMERA_RUNTIME_ROOT,'--image',(Join-Path $root 'tests/fixtures/ocr/ascii-smoke.png'),'--work',(Join-Path $work 'installed OCR private state')) 60 45
    if ($IsWindows) {
        $saved=$env:NUMERA_ROOT;$savedOrt=$env:NUMERA_ORT_LIBRARY;$savedDevice=$env:NUMERA_DEVICE
        try {
            $env:NUMERA_ROOT=$installed;$env:NUMERA_ORT_LIBRARY=Join-Path $installed 'data/runtime/onnxruntime.dll'
            Test-InstalledLaunchers $installed
            # Arithmetic does not invoke model inference; use the immutable fixture contract.
            $env:NUMERA_DEVICE=$null
            [IO.Directory]::CreateDirectory((Join-Path $work 'persistence'))|Out-Null
            Required 'installed-native-persistence' (Join-Path $root "target/debug/examples/hosted_rollback$suffix") @('--executable',(Join-Path $installed 'bin/numera-cha.exe'),'--work',(Join-Path $work 'persistence')) 90 30
        } finally { $env:NUMERA_ROOT=$saved;$env:NUMERA_ORT_LIBRARY=$savedOrt;$env:NUMERA_DEVICE=$savedDevice }
    }
    if ($failures.Count) { throw "Required hosted tiers failed: $($failures -join ', ')." }
    Write-HostedLog INFO verification 'Native CPU, protocol/review/platform/browser and distribution tiers passed; GPU and native visual human review remain explicit limits.'
} catch { $code=1;Write-HostedLog ERROR verification $_.Exception.Message }
finally {
    $owners=Join-Path $root '.ci-work/owners'
    if ((Test-Path -LiteralPath $owners) -and @(Get-ChildItem -LiteralPath $owners -File -Filter 'owner-*.json').Count) { $code=1;$failures.Add('owned-process-record-survived') }
    [IO.File]::WriteAllText((Join-Path $work 'results.json'),(@{platform=$Platform;stages=$results.ToArray();failures=$failures.ToArray();completed=($code -eq 0)}|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
    Write-HostedLog INFO cleanup "Hosted verification finished exit_code=$code."
}
exit $code
