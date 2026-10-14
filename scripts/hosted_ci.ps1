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
function Test-SourceOracle {
    $oracle=Join-Path $work 'oracle'
    $savedDevice=$env:NUMERA_DEVICE
    try {
        $env:NUMERA_DEVICE=$null
        Required 'historic-frozen-source-oracle' pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $PSScriptRoot 'source_oracle.ps1'),'-Work',$oracle) 300 120
    } finally { $env:NUMERA_DEVICE=$savedDevice }
    foreach ($name in @('arithmetic.json','sdk.json','ascii-preprocess.f32','preprocess.json','http.json','request-digests.json','contracts.json')) {
        $expected=Join-Path $root "tests/fixtures/parity/$name";$actual=Join-Path $oracle $name
        if ($name.EndsWith('.json')) {
            if ([IO.File]::ReadAllText($expected,[Text.Encoding]::UTF8).Replace("`r`n","`n") -cne [IO.File]::ReadAllText($actual,[Text.Encoding]::UTF8).Replace("`r`n","`n")) { throw "Pinned source fixture drift: $name." }
        } elseif ((Get-FileHash -LiteralPath $expected).Hash -ne (Get-FileHash -LiteralPath $actual).Hash) { throw 'Pinned tensor source fixture drift.' }
    }
    $generated=[IO.File]::ReadAllText((Join-Path $oracle 'source-tests.json'),[Text.Encoding]::UTF8)|ConvertFrom-Json
    $mapping=[IO.File]::ReadAllText((Join-Path $root 'tests/fixtures/parity/source-tests.json'),[Text.Encoding]::UTF8)|ConvertFrom-Json
    foreach ($row in $mapping) {
        if ($row.status -notin @('parity','native','retired') -or -not $row.target) { throw 'Historic source case is pending or unmapped.' }
        foreach ($target in ($row.target -split ';')) {
            $parts=$target.Trim() -split '::',2
            $path=Join-Path $root $parts[0]
            if (-not [IO.File]::Exists($path)) { throw 'Historic source mapping target is missing.' }
            if ($row.status -ne 'retired' -and $parts.Count -eq 2 -and -not [IO.File]::ReadAllText($path,[Text.Encoding]::UTF8).Contains("fn $($parts[1])(")) {
                if ($parts[0] -ne 'scripts/native_browser.py' -or $parts[1] -ne 'main') { throw 'Historic source mapped test function is missing.' }
            }
            if ($row.status -eq 'retired' -and -not $row.reason) { throw 'Historic source retirement requires its recorded reason.' }
        }
    }
    $a=@($generated|ForEach-Object {"$($_.source)|$($_.name)|$($_.source_sha256)"}|Sort-Object)
    $b=@($mapping|ForEach-Object {"$($_.source)|$($_.name)|$($_.source_sha256)"}|Sort-Object)
    if (@(Compare-Object $a $b).Count) { throw 'Historic source-test inventory drift.' }
}
function Test-InstalledLaunchers([string]$Installed) {
    # Reuse the maintained Windows launcher ownership regression and actual installed entry points.
    Required 'installed-doctor-launcher' pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $Installed 'scripts/doctor.ps1'),'-Device','cpu') 130 45
    Required 'installed-setup-no-source' pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $Installed 'scripts/setup.ps1'),'-Yes','-Engine','cpu') 60 30
    Required 'installed-stop-without-owner' pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $Installed 'scripts/stop.ps1')) 30 15
    $source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'native_ci.ps1'),[Text.Encoding]::UTF8)
    $tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseInput($source,[ref]$tokens,[ref]$errors)
    if ($errors.Count) { throw 'Maintained launcher regression cannot be parsed.' }
    $function=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Test-InstalledLaunchers'},$true)
    if (-not $function) { throw 'Maintained installed start/stop regression is unavailable.' }
    $script=Join-Path $work 'installed-launcher-check.ps1'
    $header="param([string]`$Installed,[string]`$Work,[string]`$Common)`nImport-Module `$Common -Force`nInitialize-RunLog 'installed-hosted-launchers'`n"
    [IO.File]::WriteAllText($script,$header+$function.Extent.Text+"`nTest-InstalledLaunchers `$Installed (Get-Command pwsh).Source `$Work`n",[Text.UTF8Encoding]::new($false))
    Required 'installed-start-stop-and-credential-redaction' pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',$script,'-Installed',$Installed,'-Work',$work,'-Common',(Join-Path $PSScriptRoot 'Common.psm1')) 90 30
}
try {
    $env:CARGO_BUILD_JOBS='2';$env:RUST_TEST_THREADS='1';$env:NUMERA_MAX_TEST_WORKERS='1'
    $env:NUMERA_TEST_GPU='0';$env:NUMERA_TEST_BROWSER='1';$env:NUMERA_DEVICE='cpu';$env:NUMERA_ROOT=$root
    if (-not $env:NUMERA_RUNTIME_ROOT -or -not $env:NUMERA_ORT_LIBRARY) { throw 'Verified CPU runtime/model fixture is required.' }
    $suffix=if($IsWindows){'.exe'}else{''};$pwsh=(Get-Command pwsh).Source
    $python=(Get-Command python).Source
    $browserPython=Join-Path $root $(if($IsWindows){'.ci-work/browser-env/Scripts/python.exe'}else{'.ci-work/browser-env/bin/python'})
    $expected=@('parity','preprocessing','adapters','lifecycle','gpu','cli','http_parity','core_contracts','startup','worker','task_regressions','downloads','decode','mcp','mcp_bridge','review','platform')
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
    if (-not $IsWindows) {
        Required 'posix-owned-process-regression' $python @('-I',(Join-Path $root 'desktop/ci/owner_contract.py'),'--work',(Join-Path $work 'owner-contract')) 45 15
    }
    Required 'format' cargo @('fmt','--all','--','--check') 60 30
    Required 'locked-workspace-dependencies' cargo @('fetch','--locked') 300 120
    Required 'native-debug-build' cargo @('build','--locked','-p','numera-cha-solver','--bins','--jobs','2') 600 180
    $fixture=Join-Path $work "worker-fixture$suffix"
    Required 'synthetic-native-worker-once' rustc @((Join-Path $root 'tests/fixtures/worker.rs'),'--edition=2024','--crate-type=bin','-o',$fixture) 60 30
    $env:NUMERA_FIXTURE_WORKER_EXECUTABLE=$fixture;$env:NUMERA_WORKER_EXECUTABLE=$null
    Required 'native-release-build' cargo @('build','--locked','--release','-p','numera-cha-solver','--bins','--jobs','2') 1200 240
    Required 'matching-source-and-desktop-resources' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $PSScriptRoot 'package_hosted.ps1'),'-Phase','Resources','-Platform',$Platform,'-Target',$Target) 180 60
    Required 'desktop-icon-from-approved-logo' cargo @('tauri','icon',(Join-Path $root 'resources/web/logo.png'),'--output',(Join-Path $root 'desktop/icons')) 60 30 '' (Join-Path $root 'desktop')
    $null=Stage 'clippy-all-native-and-desktop' cargo @('clippy','--locked','--workspace','--all-targets','--jobs','2','--','-D','warnings') 1200 180
    $testCapture=Join-Path $work 'native-tests.txt'
    $tests=Stage 'all-native-and-desktop-tests' cargo @('test','--locked','--workspace','--all-targets','--no-fail-fast','--jobs','2','--','--test-threads=1') 600 120 $testCapture
    if ($tests -eq 0) {
        $output=[IO.File]::ReadAllText($testCapture,[Text.Encoding]::UTF8)
        if ($output -match '[1-9][0-9]* ignored;' -or $output -notmatch 'test result: ok\.') { throw 'Native suite skipped a required case or provided no test results.' }
    }
    $null=Stage 'native-headless-browser' $browserPython @((Join-Path $PSScriptRoot 'native_browser.py'),'--executable',(Join-Path $root "target/debug/numera-cha$suffix"),'--work',(Join-Path $work 'browser')) 120 45
    if ($IsWindows) {
        $null=Stage 'windows-owned-launcher-tree' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $root 'tests/launcher_ownership.ps1')) 90 30
        Test-SourceOracle
    }
    $bundle=if($IsWindows){'nsis'}elseif($IsMacOS){'app'}else{'deb'}
    Required 'native-desktop-build-and-bundle' cargo @('tauri','build','--ci','--bundles',$bundle,'--','--locked','--jobs','2') 1200 240 '' (Join-Path $root 'desktop')
    if ($Platform -eq 'linux-x64') {
        if ($failures.Count) { throw 'Native validation failures block desktop execution and artifact creation.' }
        $deb=@(Get-ChildItem -LiteralPath (Join-Path $root 'target/release/bundle/deb') -File -Filter '*.deb')
        if ($deb.Count -ne 1) { throw 'Linux native installer discovery failed.' }
        Required 'linux-install-inspected-desktop' sudo @('-n','dpkg','-i',$deb[0].FullName) 60 30
        Required 'linux-packaged-native-webdriver' xvfb-run @('-a',$python,'-I',(Join-Path $root 'desktop/ci/desktop_webdriver.py'),'--executable','/usr/bin/numera-cha-desktop','--driver',(Join-Path $root '.ci-work/tools/bin/tauri-driver'),'--work',(Join-Path $work 'desktop-webdriver')) 120 30
    }
    Required 'final-native-distributions' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $PSScriptRoot 'package_hosted.ps1'),'-Phase','Archives','-Platform',$Platform,'-Target',$Target) 180 60
    $installed=Join-Path $work 'installed native sample'
    [IO.Compression.ZipFile]::ExtractToDirectory((Join-Path $root ".ci-work/artifacts/numera-cha-0.3.0-$Platform.zip"),$installed)
    if (-not $IsWindows) { foreach ($file in (Get-ChildItem -LiteralPath (Join-Path $installed 'bin') -File)) { [IO.File]::SetUnixFileMode($file.FullName,[IO.UnixFileMode]::UserRead -bor [IO.UnixFileMode]::UserWrite -bor [IO.UnixFileMode]::UserExecute -bor [IO.UnixFileMode]::GroupRead -bor [IO.UnixFileMode]::GroupExecute -bor [IO.UnixFileMode]::OtherRead -bor [IO.UnixFileMode]::OtherExecute) } }
    $null=Stage 'installed-cli-connector-and-real-cpu-ocr' $python @('-I',(Join-Path $root 'desktop/ci/installed_cli.py'),'--root',$installed,'--runtime-root',$env:NUMERA_RUNTIME_ROOT,'--image',(Join-Path $root 'tests/fixtures/ocr/ascii-smoke.png'),'--work',(Join-Path $work 'installed OCR private state')) 60 45
    if ($IsWindows) {
        $saved=$env:NUMERA_ROOT;$savedOrt=$env:NUMERA_ORT_LIBRARY;$savedDevice=$env:NUMERA_DEVICE
        try {
            $env:NUMERA_ROOT=$installed;$env:NUMERA_ORT_LIBRARY=Join-Path $installed 'data/runtime/onnxruntime.dll'
            Test-InstalledLaunchers $installed
            # Historic arithmetic result includes the source's default device label; no GPU inference occurs here.
            $env:NUMERA_DEVICE=$null
            Required 'copied-state-historic-rollback' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $PSScriptRoot 'source_oracle.ps1'),'-Operation','Rollback','-Executable',(Join-Path $installed 'bin/numera-cha.exe'),'-Work',(Join-Path $work 'rollback')) 180 60
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
