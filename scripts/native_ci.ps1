#requires -Version 7.0
[CmdletBinding()]
param([switch]$Gpu,[switch]$Browser,[string]$RuntimeRoot='')
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'native-ci'
$code=0;$failures=[Collections.Generic.List[string]]::new();$installed=$null;$work=$null;$results=[Collections.Generic.List[object]]::new()
function Test-InstalledLaunchers([string]$Installed,[string]$PowerShell,[string]$Work){
    $reservation=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0)
    $reservation.Start();$port=$reservation.LocalEndpoint.Port;$reservation.Stop()
    $info=[Diagnostics.ProcessStartInfo]::new();$info.FileName=$PowerShell
    foreach($argument in @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $Installed 'scripts/start.ps1'),'-Port',"$port",'-Device','cpu')){$info.ArgumentList.Add($argument)}
    $info.WorkingDirectory=$Work;$info.UseShellExecute=$false;$info.CreateNoWindow=$true
    $info.RedirectStandardInput=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
    $info.Environment['NUMERA_ROOT']=$Installed;$info.Environment['NUMERA_API_TOKEN']='owned-launcher-fixture-sentinel'
    $process=[Diagnostics.Process]::new();$process.StartInfo=$info;$started=$null;$server=$null
    $stdout=$null;$stderr=$null;$outTask=$null;$errTask=$null
    try{
        $process.Start()|Out-Null;$started=$process.StartTime;$process.StandardInput.Close()
        Write-RunLog INFO process "Installed launcher owner started pid=$($process.Id)."
        $stdout=[IO.File]::Create((Join-Path $Work 'launcher-server.out'));$stderr=[IO.File]::Create((Join-Path $Work 'launcher-server.err'))
        $outTask=$process.StandardOutput.BaseStream.CopyToAsync($stdout);$errTask=$process.StandardError.BaseStream.CopyToAsync($stderr)
        $state=Join-Path $Installed 'data/server.json';$deadline=[DateTime]::UtcNow.AddSeconds(15)
        $client=[Net.Http.HttpClient]::new();$client.Timeout=[TimeSpan]::FromSeconds(1)
        try{
            $ready=$false
            while([DateTime]::UtcNow -lt $deadline){
                if($process.HasExited){throw 'Installed server launcher exited before readiness.'}
                if([IO.File]::Exists($state)){
                    try{
                        $record=[IO.File]::ReadAllText($state,[Text.Encoding]::UTF8)|ConvertFrom-Json
                        $candidate=Get-Process -Id ([int]$record.pid) -ErrorAction Stop
                        if($candidate.StartTime.ToUniversalTime().ToString('o') -ne ([datetime]$record.started_utc).ToUniversalTime().ToString('o') -or $candidate.Path -ne (Join-Path $Installed 'bin/numera-cha.exe')){$candidate.Dispose();throw 'Installed server process identity differs.'}
                        if($server){$server.Dispose()};$server=$candidate
                        $response=$client.GetAsync("http://127.0.0.1:$port/health/live").GetAwaiter().GetResult()
                        try{if($response.IsSuccessStatusCode){$ready=$true;break}}finally{$response.Dispose()}
                    }catch{if([DateTime]::UtcNow -ge $deadline){throw}}
                }
                # Bounded readiness poll; no fixed startup sleep or unbounded wait.
                [Threading.Thread]::Sleep(50)
            }
            if(-not $ready){throw 'Installed launcher did not establish bounded loopback readiness.'}
        }finally{$client.Dispose()}
        $stateBefore=[IO.File]::ReadAllText($state,[Text.Encoding]::UTF8)
        $contender=Invoke-BoundedCommand -Executable $PowerShell -Arguments @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $Installed 'scripts/start.ps1'),'-Port',"$port",'-Device','cpu') -WallSeconds 15 -IdleSeconds 10 -WorkingDirectory $Work
        if($contender -eq 0 -or [IO.File]::ReadAllText($state,[Text.Encoding]::UTF8) -ne $stateBefore){throw 'Installed live ownership contender did not fail without state mutation.'}
        $stop=Invoke-BoundedCommand -Executable $PowerShell -Arguments @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $Installed 'scripts/stop.ps1')) -WallSeconds 15 -IdleSeconds 10 -WorkingDirectory $Work
        if($stop -ne 0 -or -not $process.WaitForExit(5000) -or -not $server.WaitForExit(5000)){throw 'Installed launcher stop did not reap its owned processes.'}
        if([IO.File]::Exists($state)){throw 'Installed launcher ownership record remained after confirmed stop.'}
        if(-not $outTask.Wait(5000) -or -not $errTask.Wait(5000)){throw 'Installed launcher streams remained open.'}
        $stdout.Dispose();$stdout=$null;$stderr.Dispose();$stderr=$null
        foreach($file in (Get-ChildItem -LiteralPath (Join-Path $Installed 'logs') -File)){
            if([IO.File]::ReadAllText($file.FullName,[Text.Encoding]::UTF8).Contains('owned-launcher-fixture-sentinel')){throw 'Synthetic launcher credential leaked to file logging.'}
        }
        foreach($path in @((Join-Path $Work 'launcher-server.out'),(Join-Path $Work 'launcher-server.err'))){
            if([IO.File]::ReadAllText($path,[Text.Encoding]::UTF8).Contains('owned-launcher-fixture-sentinel')){throw 'Synthetic launcher credential leaked to captured output.'}
            [IO.File]::Delete($path)
        }
    }finally{
        if($started){if(-not $process.HasExited){$process.Kill($true);if(-not $process.WaitForExit(5000)){throw 'Installed launcher owner cleanup failed.'}}}
        if($server){if(-not $server.HasExited){if($server.Path -ne (Join-Path $Installed 'bin/numera-cha.exe') -or $server.StartTime -lt $started){throw 'Installed native cleanup identity mismatch.'};$server.Kill($true);if(-not $server.WaitForExit(5000)){throw 'Installed native server cleanup failed.'}};$server.Dispose()}
        if($outTask){$null=$outTask.Wait(5000)};if($errTask){$null=$errTask.Wait(5000)}
        if($stdout){$stdout.Dispose()};if($stderr){$stderr.Dispose()};$process.Dispose()
    }
}
try{
    $root=Get-ProjectRoot
    Initialize-NativeEnvironment
    $cargo=Get-RustTool cargo
    if(-not $RuntimeRoot){$RuntimeRoot=if($env:NUMERA_RUNTIME_ROOT){$env:NUMERA_RUNTIME_ROOT}else{$root}}
    $RuntimeRoot=[IO.Path]::GetFullPath($RuntimeRoot)
    $env:NUMERA_RUNTIME_ROOT=$RuntimeRoot;$env:RUST_TEST_THREADS='1';$env:NUMERA_MAX_TEST_WORKERS='1'
    $env:NUMERA_TEST_GPU=if($Gpu){'1'}else{'0'};$env:NUMERA_TEST_BROWSER=if($Browser){'1'}else{'0'}
    if(-not $Gpu -or -not $Browser){throw 'Final native migration CI requires both -Gpu and -Browser; use focused native_check for development.'}
    $work=Join-Path $root '.ci-work/native-ci';[IO.Directory]::CreateDirectory($work)|Out-Null
    $results=[Collections.Generic.List[object]]::new()
    function Run-Stage([string]$Name,[string]$Executable,[string[]]$Arguments,[int]$Wall=120,[int]$Idle=45,[string]$Capture='',[string]$Cwd=$root){
        $watch=[Diagnostics.Stopwatch]::StartNew();$status=1
        try{$status=Invoke-BoundedCommand -Executable $Executable -Arguments $Arguments -WallSeconds $Wall -IdleSeconds $Idle -CapturePath $Capture -WorkingDirectory $Cwd}catch{Write-RunLog ERROR verification "Stage=$Name failed; captured safe diagnostic follows.";Write-RunLog ERROR verification $_.Exception.Message}
        $results.Add(@{name=$Name;exit_code=$status;duration_seconds=$watch.Elapsed.TotalSeconds})
        if($status -ne 0){$failures.Add($Name)}
        return $status
    }
    foreach($target in @('parity','preprocessing','adapters','lifecycle','gpu','cli','http_parity','core_contracts','startup','worker','task_regressions','downloads','decode')){
        if(-not [IO.File]::Exists((Join-Path $root "tests/$target.rs"))){throw "Required native test tier is missing: $target."}
    }
    $browserScript=Join-Path $PSScriptRoot 'native_browser.py'
    if(-not [IO.File]::Exists($browserScript)){throw 'Required native headless browser harness is missing.'}
    $rollbackScript=Join-Path $PSScriptRoot 'native_rollback.py'
    if(-not [IO.File]::Exists($rollbackScript)){throw 'Required copied-state source-runtime rollback harness is missing.'}
    $mappingPath=Join-Path $root 'tests/fixtures/parity/source-tests.json'
    $mapping=[IO.File]::ReadAllText($mappingPath,[Text.Encoding]::UTF8)|ConvertFrom-Json
    if(-not $mapping -or @($mapping|Where-Object{$_.status -notin @('parity','native','retired') -or -not $_.target}).Count){throw 'Source test mapping has pending or unmapped cases; completion is blocked.'}
    foreach($row in $mapping){
        foreach($target in ($row.target -split ';')){
            $parts=$target.Trim() -split '::',2
            $path=Join-Path $root $parts[0]
            if(-not [IO.File]::Exists($path)){throw 'Mapped native coverage target does not exist.'}
            if($row.status -ne 'retired' -and $parts.Count -eq 2 -and -not [IO.File]::ReadAllText($path,[Text.Encoding]::UTF8).Contains("fn $($parts[1])(")){
                if($parts[0] -ne 'scripts/native_browser.py' -or $parts[1] -ne 'main'){throw 'Mapped native test function does not exist.'}
            }
            if($row.status -eq 'retired' -and -not $row.reason){throw 'Retired source-only coverage requires explicit owner-approved reason.'}
        }
    }
    $utf8=[Text.UTF8Encoding]::new($false,$true)
    $tracked=& git -C $root ls-files
    if($LASTEXITCODE -ne 0){throw 'Tracked text inventory is unavailable.'}
    foreach($relative in $tracked){
        $path=Join-Path $root $relative
        if([IO.Path]::GetExtension($relative) -in @('.rs','.cs','.py','.ps1','.psm1','.md','.json','.toml','.lock','.yml','.yaml','.html','.css','.js') -or [IO.Path]::GetFileName($relative) -in @('.gitignore','.graphifyignore','.python-version','LICENSE')){
            $text=$utf8.GetString([IO.File]::ReadAllBytes($path))
            if([IO.Path]::GetExtension($relative) -eq '.rs' -and ($text -split "`n").Count -gt 800){throw 'Native source file exceeds the supported line ceiling.'}
        }
    }
    $pwsh=(Get-Command pwsh -ErrorAction Stop).Source
    $null=Run-Stage 'launcher-owned-process-tree' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $root 'tests/launcher_ownership.ps1')) 90 30
    $null=Run-Stage 'format' $cargo @('fmt','--all','--','--check') 60 30
    $null=Run-Stage 'clippy' $cargo @('clippy','--locked','--all-targets','--jobs','2','--','-D','warnings') 300 120
    Copy-NativeRuntime -SourceRoot $RuntimeRoot
    $oracle=Join-Path $work ('oracle-'+[guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($oracle)|Out-Null
    try{
        if((Run-Stage 'source-oracle' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $PSScriptRoot 'source_oracle.ps1'),'-Work',$oracle) 180 60) -eq 0){
            foreach($name in @('arithmetic.json','sdk.json','ascii-preprocess.f32','preprocess.json','http.json','request-digests.json','contracts.json')){
                $expected=Join-Path $root "tests/fixtures/parity/$name";$actual=Join-Path $oracle $name
                if(-not [IO.File]::Exists($actual)){$failures.Add("oracle-drift:$name");continue}
                if($name.EndsWith('.json')){
                    $sourceText=[IO.File]::ReadAllText($expected,[Text.Encoding]::UTF8).Replace("`r`n","`n")
                    $generated=[IO.File]::ReadAllText($actual,[Text.Encoding]::UTF8).Replace("`r`n","`n")
                    if($sourceText -cne $generated){$failures.Add("oracle-drift:$name")}
                }elseif((Get-FileHash -LiteralPath $expected -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $actual -Algorithm SHA256).Hash){$failures.Add("oracle-drift:$name")}
            }
            $inventory=[IO.File]::ReadAllText((Join-Path $oracle 'source-tests.json'),[Text.Encoding]::UTF8)|ConvertFrom-Json
            $expected=@($mapping|ForEach-Object{"$($_.source)|$($_.name)|$($_.source_sha256)"}|Sort-Object)
            $actual=@($inventory|ForEach-Object{"$($_.source)|$($_.name)|$($_.source_sha256)"}|Sort-Object)
            if(@(Compare-Object $expected $actual).Count){$failures.Add('source-test-inventory-drift')}
        }
    }finally{if([IO.Directory]::Exists($oracle)){[IO.Directory]::Delete($oracle,$true)}}
    $python=Get-ProjectPython
    $build=Run-Stage 'native-debug-build' $cargo @('build','--locked','--bin','numera-cha','--jobs','2') 300 120
    $env:NUMERA_WORKER_EXECUTABLE=Join-Path $root 'target/debug/numera-cha.exe'
    if($build -eq 0){
        $env:NUMERA_WORKER_EXECUTABLE=$null
        $null=Run-Stage 'all-native-tests' $cargo @('test','--locked','--all-targets','--no-fail-fast','--jobs','2','--','--test-threads=1') 420 120
        $env:NUMERA_WORKER_EXECUTABLE=$null
        $browserOutput=Join-Path $work 'browser-output.txt'
        $browserWork=Join-Path $work ('browser-'+[guid]::NewGuid().ToString('N'))
        $browserStatus=Run-Stage 'native-headless-browser' $python @($browserScript,'--executable',(Join-Path $root 'target/debug/numera-cha.exe'),'--work',$browserWork) 120 45 $browserOutput
        if($browserStatus -eq 0 -and [IO.Directory]::Exists($browserWork)){[IO.Directory]::Delete($browserWork,$true)}
    }
    $release=Run-Stage 'native-release-build' $cargo @('build','--locked','--release','--bin','numera-cha','--jobs','2') 1200 180
    if($release -eq 0){
        $packageDirectory=Join-Path $work ('packages-'+[guid]::NewGuid().ToString('N'))
        $packageOutput=Join-Path $work 'package-output.txt'
        $pwsh=(Get-Command pwsh -ErrorAction Stop).Source
        $package=Run-Stage 'native-package' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $PSScriptRoot 'native_package.ps1'),'-OutputDirectory',$packageDirectory) 120 45 $packageOutput
        if($package -eq 0){
            $archive=[IO.File]::ReadAllText($packageOutput,[Text.Encoding]::UTF8).Trim()
            $installed=Join-Path $work ('installed native sample-'+[guid]::NewGuid().ToString('N'))
            [IO.Compression.ZipFile]::ExtractToDirectory($archive,$installed)
            $savedRoot=$env:NUMERA_ROOT;$savedDll=$env:NUMERA_ORT_LIBRARY
            try{
                $env:NUMERA_ROOT=$installed;$env:NUMERA_ORT_LIBRARY=Join-Path $installed 'data/runtime/onnxruntime.dll'
                $native=Join-Path $installed 'bin/numera-cha.exe';$capture=Join-Path $work 'installed-result.json'
                $smoke=Run-Stage 'installed-native-arithmetic' $native @('--device','cpu','recognize','--text','7+5') 45 20 $capture $work
                if($smoke -eq 0){
                    $result=[IO.File]::ReadAllText($capture,[Text.Encoding]::UTF8)|ConvertFrom-Json
                    if($result.answer -ne '12' -or $result.server_verified){$failures.Add('installed-native-answer')}
                }
                $null=Run-Stage 'installed-doctor-launcher' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $installed 'scripts/doctor.ps1'),'-Device','cpu') 130 45 (Join-Path $work 'installed-doctor.json') $work
                $null=Run-Stage 'installed-setup-no-source' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $installed 'scripts/setup.ps1'),'-Yes','-Engine','cpu') 60 30 '' $work
                $null=Run-Stage 'installed-stop-without-owner' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $installed 'scripts/stop.ps1')) 30 15 '' $work
                $rollbackWork=Join-Path $work ('rollback-'+[guid]::NewGuid().ToString('N'))
                $rollbackStatus=Run-Stage 'copied-source-runtime-rollback' $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $PSScriptRoot 'source_oracle.ps1'),'-Operation','Rollback','-Executable',$native,'-Work',$rollbackWork) 180 60 (Join-Path $work 'rollback-result.txt') $root
                if($rollbackStatus -eq 0 -and [IO.Directory]::Exists($rollbackWork)){[IO.Directory]::Delete($rollbackWork,$true)}
                $launcherWatch=[Diagnostics.Stopwatch]::StartNew();$launcherStatus=0
                try{Test-InstalledLaunchers $installed $pwsh $work}catch{$launcherStatus=1;$failures.Add('installed-start-stop');Write-RunLog ERROR verification $_.Exception.Message}
                $results.Add(@{name='installed-start-stop';exit_code=$launcherStatus;duration_seconds=$launcherWatch.Elapsed.TotalSeconds})
                if(@(Get-ChildItem -LiteralPath (Join-Path $installed 'logs') -File -Filter '*.log').Count -lt 3){$failures.Add('installed-launcher-file-logs')}
            }finally{$env:NUMERA_ROOT=$savedRoot;$env:NUMERA_ORT_LIBRARY=$savedDll}
        }
    }
    [IO.File]::WriteAllText((Join-Path $work 'results.json'),(@{stages=$results.ToArray();failures=$failures.ToArray()}|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
    if($failures.Count){throw "Native verification failed in $($failures.Count) stage(s). Inspect results.json; no retirement is authorized."}
    Write-RunLog INFO verification 'Native migration gates completed; exact-SHA publication and final cleanup remain integrator-owned.'
}catch{if($code -eq 0){$code=1};Write-RunLog ERROR verification $_.Exception.Message
    if($work -and [IO.Directory]::Exists($work)){
        [IO.File]::WriteAllText((Join-Path $work 'results.json'),(@{stages=$results.ToArray();failures=$failures.ToArray();completed=$false}|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
    }
}
finally{if($code -eq 0 -and $installed -and [IO.Directory]::Exists($installed)){[IO.Directory]::Delete($installed,$true)};Write-RunLog INFO cleanup "Native CI finished exit_code=$code."}
exit $code
