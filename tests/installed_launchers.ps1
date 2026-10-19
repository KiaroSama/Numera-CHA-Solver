#requires -Version 7.0
param([Parameter(Mandatory)][string]$Installed,[Parameter(Mandatory)][string]$Work,[Parameter(Mandatory)][string]$Common)
Import-Module $Common -Force
Initialize-RunLog "installed-launchers"
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
Test-InstalledLaunchers $Installed (Get-Command pwsh).Source $Work
