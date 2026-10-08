#requires -Version 7.0
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:Root = Split-Path -Parent $PSScriptRoot
$script:LogPath = $null

function Initialize-RunLog([string]$Name) {
    try {
        $directory = Join-Path $script:Root 'logs'
        [IO.Directory]::CreateDirectory($directory) | Out-Null
        $stamp = [DateTime]::UtcNow.ToString('yyyy-MM-dd_HH-mm-ss')
        $script:LogPath = Join-Path $directory "$($Name)_$($stamp)_UTC_$([guid]::NewGuid().ToString('N').Substring(0,8)).log"
        [IO.File]::WriteAllText($script:LogPath, '', [Text.UTF8Encoding]::new($false))
        Write-RunLog INFO startup 'Diagnostic logging initialized.'
    } catch { $script:LogPath=$null; [Console]::Error.WriteLine('File logging unavailable; stderr fallback active.') }
}
function Write-RunLog([ValidateSet('INFO','WARNING','ERROR','DEBUG')][string]$Level, [string]$Component, [string]$Message) {
    $safe = $Message -replace '(?i)Bearer\s+\S+', 'Bearer [REDACTED]'
    $safe = $safe -replace '(?i)(["'']?(?:token|secret|password|cookies?|authorization|api[_-]?key|response)["'']?\s*[:=]\s*)(?:"(?:\\.|[^"\\])*"|''(?:\\.|[^''\\])*''|[^\s,;}]+)', '$1[REDACTED]'
    $safe = ($safe -replace 'https?://\S+', '[URL REDACTED]').Replace("`r",'\r').Replace("`n",'\n')
    $line="[$([DateTime]::UtcNow.ToString('yyyy-MM-dd HH:mm:ss')) UTC] [$Level] [$Component] $safe"
    [Console]::Error.WriteLine($line)
    if($script:LogPath){try{[IO.File]::AppendAllText($script:LogPath,$line+[Environment]::NewLine,[Text.UTF8Encoding]::new($false))}catch{$script:LogPath=$null;[Console]::Error.WriteLine('Writing diagnostic log failed; stderr fallback active.')}}
}
function Get-ProjectRoot { $script:Root }
function Get-ProjectPython {
    $python=if($env:NUMERA_ORACLE_PYTHON){$env:NUMERA_ORACLE_PYTHON}else{Join-Path $script:Root '.venv/Scripts/python.exe'}
    if(-not [IO.File]::Exists($python)){throw 'Source-oracle environment is missing. It is needed only during migration verification.'}
    $python
}
function Get-NativeExecutable {
    foreach($relative in @('bin/numera-cha.exe','target/release/numera-cha.exe','target/debug/numera-cha.exe')){
        $path=Join-Path $script:Root $relative
        if([IO.File]::Exists($path)){return $path}
    }
    throw 'Native executable is missing. Run scripts/setup.ps1 first or extract the inspected native package.'
}
function Get-RustTool([ValidateSet('cargo','rustc','rustfmt','clippy-driver','rustdoc')][string]$Name) {
    $rustup=(Get-Command rustup -ErrorAction Stop).Source
    $path=(& $rustup which $Name).Trim()
    if($LASTEXITCODE -ne 0 -or -not [IO.File]::Exists($path)){throw "Selected Rust component is unavailable: $Name. Install prerequisites in your approved tools location."}
    $path
}
function Initialize-NativeEnvironment {
    $env:NUMERA_ROOT=$script:Root
    $env:CARGO_BUILD_JOBS='2'
    $env:RUSTC=Get-RustTool rustc
    $env:RUSTDOC=Get-RustTool rustdoc
    $temp=Join-Path $script:Root '.ci-cache/tmp'
    [IO.Directory]::CreateDirectory($temp)|Out-Null
    $env:TEMP=$temp; $env:TMP=$temp
}
function Copy-NativeRuntime {
    param([string]$SourceRoot=$script:Root)
    $destination=Join-Path $script:Root 'data/runtime'
    $names=@('onnxruntime.dll','onnxruntime_providers_shared.dll','onnxruntime_providers_cuda.dll')
    $source=Join-Path $SourceRoot 'data/runtime'
    if(-not [IO.File]::Exists((Join-Path $source 'onnxruntime.dll'))){$source=Join-Path $SourceRoot '.venv/Lib/site-packages/onnxruntime/capi'}
    foreach($name in $names){
        $path=Join-Path $source $name
        if(-not [IO.File]::Exists($path) -or ((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Reviewed native runtime DLLs are missing or linked. Supply the approved local ONNX Runtime; no download was attempted.'}
    }
    $licenseSource=Join-Path $SourceRoot 'data/runtime/licenses'
    if(-not [IO.File]::Exists((Join-Path $licenseSource 'LICENSE'))){$licenseSource=Join-Path $SourceRoot '.venv/Lib/site-packages/onnxruntime'}
    foreach($name in @('LICENSE','ThirdPartyNotices.txt')){if(-not [IO.File]::Exists((Join-Path $licenseSource $name))){throw 'Native runtime redistribution notices are unavailable; staging is blocked.'}}
    [IO.Directory]::CreateDirectory($destination)|Out-Null
    foreach($name in $names){
        $from=Join-Path $source $name; $to=Join-Path $destination $name
        if(-not [string]::Equals([IO.Path]::GetFullPath($from),[IO.Path]::GetFullPath($to),[StringComparison]::OrdinalIgnoreCase)){
            $temporary="$to.$([guid]::NewGuid().ToString('N')).tmp"
            try{[IO.File]::Copy($from,$temporary,$false);[IO.File]::Move($temporary,$to,$true)}finally{if([IO.File]::Exists($temporary)){[IO.File]::Delete($temporary)}}
        }
    }
    $licenseDestination=Join-Path $destination 'licenses'
    [IO.Directory]::CreateDirectory($licenseDestination)|Out-Null
    foreach($name in @('LICENSE','ThirdPartyNotices.txt')){
        $from=Join-Path $licenseSource $name; $to=Join-Path $licenseDestination $name
        if(-not [string]::Equals([IO.Path]::GetFullPath($from),[IO.Path]::GetFullPath($to),[StringComparison]::OrdinalIgnoreCase)){[IO.File]::Copy($from,$to,$true)}
    }
    $env:NUMERA_ORT_LIBRARY=Join-Path $destination 'onnxruntime.dll'
    $configuration=Join-Path $SourceRoot 'data/runtime-config.json'
    if(-not $env:NUMERA_DLL_DIRECTORY -and [IO.File]::Exists($configuration)){
        $saved=[IO.File]::ReadAllText($configuration,[Text.Encoding]::UTF8)|ConvertFrom-Json
        if($saved.dll_directory -isnot [string]){throw 'Saved runtime library configuration is invalid.'}
        $env:NUMERA_DLL_DIRECTORY=$saved.dll_directory
    }
    Write-RunLog INFO runtime 'Approved native runtime staged locally; no Python application or model assets copied.'
}
function Read-ClosedCapture([string]$Path) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    while($true){
        try{$stream=[IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read);break}
        catch [IO.IOException]{
            if(($_.Exception.GetBaseException().HResult -band 0xffff) -ne 32){throw}
            if($watch.ElapsedMilliseconds -ge 5000){throw 'Owned capture writer handles did not close.'}
            # Confirm actual writer-handle release; job accounting alone can precede kernel handle cleanup.
            [Threading.Thread]::Sleep(10)
        }
    }
    try{
        $reader=[IO.StreamReader]::new($stream,[Text.Encoding]::UTF8,$true)
        try{return $reader.ReadToEnd()}finally{$reader.Dispose()}
    }finally{$stream.Dispose()}
}
function Invoke-BoundedCommand {
    param([Parameter(Mandatory)][string]$Executable,[string[]]$Arguments=@(),[ValidateRange(1,86400)][int]$WallSeconds=300,[ValidateRange(1,86400)][int]$IdleSeconds=60,[string]$WorkingDirectory=$script:Root,[string]$CapturePath='',[string]$OwnershipPath='')
    if(-not ('Numera.Launcher.NativeProcessOwner' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'NativeProcessOwner.cs')}
    if(-not [IO.Path]::IsPathRooted($Executable)){$Executable=(Get-Command $Executable -CommandType Application -ErrorAction Stop).Source}
    $temporary=Join-Path $script:Root '.ci-cache/tmp';[IO.Directory]::CreateDirectory($temporary)|Out-Null
    $environment=[Collections.Generic.Dictionary[string,string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach($entry in [Environment]::GetEnvironmentVariables().GetEnumerator()){$environment[[string]$entry.Key]=[string]$entry.Value}
    $environment['TEMP']=$temporary;$environment['TMP']=$temporary
    $outputPath=Join-Path $temporary ([guid]::NewGuid().ToString('N')+'.out');$errorPath=Join-Path $temporary ([guid]::NewGuid().ToString('N')+'.err')
    $owner=$null;$ownedStart=$null;$stateCreated=$false;$treeEmpty=$false;$exitCode=1
    $watch=[Diagnostics.Stopwatch]::StartNew();$lastProgress=0.0;$lastCpu=0.0;$lastBytes=0L
    try{
        $owner=[Numera.Launcher.NativeProcessOwner]::new($Executable,$Arguments,$WorkingDirectory,$environment,$outputPath,$errorPath)
        $ownedStart=$owner.StartTimeUtc.ToString('o')
        if($OwnershipPath){
            $record=@{pid=$owner.Id;started_utc=$ownedStart;root=$script:Root;executable=[IO.Path]::GetFullPath($Executable);port=$env:NUMERA_SERVER_PORT}
            $bytes=[Text.UTF8Encoding]::new($false).GetBytes(($record|ConvertTo-Json -Depth 10 -Compress))
            $stateFile=[IO.File]::Open($OwnershipPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
            $stateCreated=$true
            try{$stateFile.Write($bytes);$stateFile.Flush($true)}finally{$stateFile.Dispose()}
        }
        Write-RunLog INFO process "Owned job root recorded pid=$($owner.Id)."
        $owner.Resume()
        while(-not $owner.WaitForExit(250)){
            $cpu=$owner.CpuSeconds;$bytes=(Get-Item -LiteralPath $outputPath).Length+(Get-Item -LiteralPath $errorPath).Length
            if($bytes -gt 32MB){throw 'Owned command output exceeded the captured-output safety ceiling.'}
            if($cpu -gt $lastCpu+0.1 -or $bytes -gt $lastBytes){$lastProgress=$watch.Elapsed.TotalSeconds;$lastCpu=$cpu;$lastBytes=$bytes}
            if($watch.Elapsed.TotalSeconds -gt $WallSeconds -or $watch.Elapsed.TotalSeconds-$lastProgress -gt $IdleSeconds){throw 'Owned process exceeded its wall or progress ceiling.'}
        }
        $exitCode=$owner.ExitCode
        # A root exit never releases ownership: terminate detached descendants and prove the job empty first.
        $owner.StopAndConfirm(5000);$treeEmpty=$true
        if((Get-Item -LiteralPath $outputPath).Length+(Get-Item -LiteralPath $errorPath).Length -gt 32MB){throw 'Owned command output exceeded the captured-output safety ceiling.'}
        $stdout=Read-ClosedCapture $outputPath
        if($CapturePath){[IO.File]::WriteAllText($CapturePath,$stdout,[Text.UTF8Encoding]::new($false))}else{[Console]::Out.Write($stdout)}
        [Console]::Error.Write((Read-ClosedCapture $errorPath))
        Write-RunLog INFO process "Owned process completed exit_code=$exitCode active_processes=0."
    }finally{
        $cleanupFailure=$false
        try{
            if($owner -and -not $treeEmpty){
                try{$owner.StopAndConfirm(5000);$treeEmpty=$true}catch{$cleanupFailure=$true;Write-RunLog ERROR cleanup 'Owned job cleanup could not be confirmed.'}
            }
            if($stateCreated -and $treeEmpty -and [IO.File]::Exists($OwnershipPath)){
                try{
                    $record=[IO.File]::ReadAllText($OwnershipPath,[Text.Encoding]::UTF8)|ConvertFrom-Json
                    if($record.pid -eq $owner.Id -and ([DateTime]$record.started_utc).ToUniversalTime() -eq $owner.StartTimeUtc){[IO.File]::Delete($OwnershipPath)}
                }catch{$cleanupFailure=$true;Write-RunLog ERROR cleanup 'Owned state cleanup could not be confirmed.'}
            }
            if(-not $cleanupFailure){
                foreach($path in @($outputPath,$errorPath)){
                    try{if([IO.File]::Exists($path)){[IO.File]::Delete($path)}}catch{$cleanupFailure=$true;Write-RunLog ERROR cleanup 'Owned capture cleanup could not be confirmed.'}
                }
            }
        }finally{if($owner){$owner.Dispose()}}
        if($cleanupFailure){throw 'Owned process cleanup could not be confirmed; this run failed.'}
    }
    return $exitCode
}
Export-ModuleMember -Function Initialize-RunLog,Write-RunLog,Get-ProjectRoot,Get-ProjectPython,Get-NativeExecutable,Get-RustTool,Initialize-NativeEnvironment,Copy-NativeRuntime,Invoke-BoundedCommand
