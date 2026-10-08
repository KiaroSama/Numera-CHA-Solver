#requires -Version 7.0
[CmdletBinding()]
param([ValidateSet('Check','Root','Worker')][string]$Mode='Check',[string]$Fixture='', [string]$Signal='', [string]$Case='',[string]$Value='')
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$utf8=[Text.UTF8Encoding]::new($false)
$powerShell=Join-Path $PSHOME 'pwsh.exe'

if($Mode -eq 'Worker'){
    $ready=[Threading.EventWaitHandle]::OpenExisting($Signal)
    $hold=[Threading.EventWaitHandle]::new($false,[Threading.EventResetMode]::ManualReset)
    try{
        $current=[Diagnostics.Process]::GetCurrentProcess()
        try{$identity=@{pid=$PID;started_utc=$current.StartTime.ToUniversalTime().ToString('o');executable=$current.MainModule.FileName}}finally{$current.Dispose()}
        [IO.File]::WriteAllText((Join-Path $Fixture 'worker.json'),($identity|ConvertTo-Json -Depth 10 -Compress),$utf8)
        $null=$ready.Set()
        if($hold.WaitOne(15000)){throw 'Fixture hold unexpectedly signaled.'}
    }finally{$hold.Dispose();$ready.Dispose()}
    exit 0
}
if($Mode -eq 'Root'){
    $current=[Diagnostics.Process]::GetCurrentProcess()
    try{$identity=@{pid=$PID;started_utc=$current.StartTime.ToUniversalTime().ToString('o');executable=$current.MainModule.FileName}}finally{$current.Dispose()}
    [IO.File]::WriteAllText((Join-Path $Fixture 'root.json'),($identity|ConvertTo-Json -Depth 10 -Compress),$utf8)
    $record=[IO.File]::ReadAllText((Join-Path $Fixture 'owner.json'),[Text.Encoding]::UTF8)|ConvertFrom-Json
    if($record.pid -ne $PID -or ([DateTime]$record.started_utc).ToUniversalTime() -ne ([DateTime]$identity.started_utc).ToUniversalTime()){throw 'Root executed before its identity record was published.'}
    if($Case -eq 'capture'){
        [Console]::OutputEncoding=$utf8
        [Console]::Out.Write($Value+[char]0x03bb)
        [Console]::Error.Write('owned-stderr')
        exit 19
    }
    $ready=[Threading.EventWaitHandle]::OpenExisting($Signal)
    $child=[Diagnostics.Process]::new()
    try{
        $info=[Diagnostics.ProcessStartInfo]::new();$info.FileName=$powerShell
        foreach($argument in @('-NoLogo','-NoProfile','-NonInteractive','-File',$PSCommandPath,'-Mode','Worker','-Fixture',$Fixture,'-Signal',$Signal)){$info.ArgumentList.Add($argument)}
        $info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardInput=$true
        $info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
        $child.StartInfo=$info;$null=$child.Start();$child.StandardInput.Close()
        if(-not $ready.WaitOne(5000)){throw 'Descendant did not signal fixture readiness.'}
        [Console]::Out.Write('descendant-ready')
        if($Case -eq 'timeout'){
            $hold=[Threading.EventWaitHandle]::new($false,[Threading.EventResetMode]::ManualReset)
            try{$null=$hold.WaitOne(15000)}finally{$hold.Dispose()}
        }
    }finally{$child.Dispose();$ready.Dispose()}
    exit 19
}

Import-Module (Join-Path (Split-Path -Parent $PSScriptRoot) 'scripts/Common.psm1') -Force
Initialize-RunLog 'launcher-ownership'
$root=Get-ProjectRoot
$work=Join-Path $root ('.ci-cache/tmp/launcher ownership '+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($work)|Out-Null
function Assert-OwnedStopped([string]$Path){
    if(-not [IO.File]::Exists($Path)){throw 'Owned fixture identity was not recorded.'}
    $identity=[IO.File]::ReadAllText($Path,[Text.Encoding]::UTF8)|ConvertFrom-Json
    $process=Get-Process -Id ([int]$identity.pid) -ErrorAction SilentlyContinue
    if($process){try{if($process.StartTime.ToUniversalTime() -eq ([DateTime]$identity.started_utc).ToUniversalTime() -and -not $process.HasExited){throw 'An owned fixture process survived command completion.'}}finally{$process.Dispose()}}
}
function Clear-OwnedFixture([string]$Directory){
    foreach($name in @('root.json','worker.json')){
        $path=Join-Path $Directory $name
        if(-not [IO.File]::Exists($path)){continue}
        $identity=[IO.File]::ReadAllText($path,[Text.Encoding]::UTF8)|ConvertFrom-Json
        $process=Get-Process -Id ([int]$identity.pid) -ErrorAction SilentlyContinue
        if($process){
            try{
                if($process.StartTime.ToUniversalTime() -eq ([DateTime]$identity.started_utc).ToUniversalTime()){
                    if($process.Path -ne $identity.executable){throw 'Owned fixture cleanup identity mismatch.'}
                    if(-not $process.HasExited){$process.Kill($true);if(-not $process.WaitForExit(5000)){throw 'Owned fixture cleanup failed.'}}
                }
            }finally{$process.Dispose()}
        }
    }
}
$exitCode=0
try{
    foreach($caseName in @('root-exit','timeout','capture','state-collision')){
        $fixtureDirectory=Join-Path $work $caseName;[IO.Directory]::CreateDirectory($fixtureDirectory)|Out-Null
        $eventName='Local\NumeraLauncherFixture'+[guid]::NewGuid().ToString('N')
        $ready=[Threading.EventWaitHandle]::new($false,[Threading.EventResetMode]::ManualReset,$eventName)
        $state=Join-Path $fixtureDirectory 'owner.json';$capture=Join-Path $fixtureDirectory 'capture.txt'
        $value='space "quoted" slash\ trailing\'
        $arguments=@('-NoLogo','-NoProfile','-NonInteractive','-File',$PSCommandPath,'-Mode','Root','-Fixture',$fixtureDirectory,'-Signal',$eventName,'-Case',$caseName,'-Value',$value)
        try{
            if($caseName -eq 'state-collision'){[IO.File]::WriteAllText($state,'{"sentinel":"keep"}',$utf8)}
            $failed=$false;$failure='';$status=-1
            try{
                $wall=if($caseName -eq 'timeout'){6}else{12}
                $status=Invoke-BoundedCommand -Executable $powerShell -Arguments $arguments -WallSeconds $wall -IdleSeconds 10 -WorkingDirectory $work -CapturePath $capture -OwnershipPath $state
            }catch{$failed=$true;$failure=$_.Exception.Message}
            if($caseName -eq 'state-collision'){
                if(-not $failed -or [IO.File]::ReadAllText($state,[Text.Encoding]::UTF8) -ne '{"sentinel":"keep"}' -or [IO.File]::Exists((Join-Path $fixtureDirectory 'root.json'))){throw 'Ownership collision executed a child or changed existing state.'}
            }else{
                Assert-OwnedStopped (Join-Path $fixtureDirectory 'root.json')
                if($caseName -ne 'capture'){Assert-OwnedStopped (Join-Path $fixtureDirectory 'worker.json')}
                if([IO.File]::Exists($state)){throw 'Ownership state remained after confirmed job cleanup.'}
                if($caseName -eq 'timeout'){
                    if(-not $failed -or $failure -notlike '*wall or progress ceiling*'){throw 'Timeout did not report its ceiling failure.'}
                }else{
                    if($failed -or $status -ne 19){throw "Root exit code was not preserved: $failure"}
                    $expected=if($caseName -eq 'capture'){$value+[char]0x03bb}else{'descendant-ready'}
                    if([IO.File]::ReadAllText($capture,[Text.Encoding]::UTF8) -cne $expected){throw 'UTF-8 argument or captured output changed.'}
                }
            }
            Write-RunLog INFO verification "Launcher ownership case=$caseName passed."
        }finally{try{Clear-OwnedFixture $fixtureDirectory}finally{$ready.Dispose()}}
    }
}catch{$exitCode=1;Write-RunLog ERROR verification $_.Exception.Message}
finally{
    foreach($directory in [IO.Directory]::EnumerateDirectories($work)){Clear-OwnedFixture $directory}
    [IO.Directory]::Delete($work,$true)
    Write-RunLog INFO cleanup "Launcher ownership fixtures closed exit_code=$exitCode."
}
exit $exitCode
