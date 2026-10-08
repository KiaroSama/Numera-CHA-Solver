#requires -Version 7.0
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'stop'
$code=0
try{
    $root=Get-ProjectRoot;$state=Join-Path $root 'data/server.json'
    if(-not [IO.File]::Exists($state)){Write-RunLog INFO cleanup 'No owned server record exists.';exit 0}
    $record=[IO.File]::ReadAllText($state,[Text.Encoding]::UTF8)|ConvertFrom-Json
    if($record.root -ne $root){throw 'Ownership root does not match; no process was stopped.'}
    if($record.PSObject.Properties.Name -contains 'pid'){
        $process=Get-Process -Id ([int]$record.pid) -ErrorAction SilentlyContinue
        if($process){
            $start=$process.StartTime.ToUniversalTime().ToString('o')
            $native=[IO.Path]::GetFullPath((Get-NativeExecutable))
            if($start -ne ([datetime]$record.started_utc).ToUniversalTime().ToString('o') -or $record.executable -ne $native -or $process.Path -ne $native){throw 'Ownership identity does not match; no process was stopped.'}
            $process.Kill($true)
            if(-not $process.WaitForExit(5000)){throw 'Owned server termination could not be confirmed.'}
            Write-RunLog INFO cleanup "Stopped owned native server pid=$($record.pid)."
        }
    }else{
        # Keep the old launch record readable during rollback; never kill its launcher shell.
        $owner=Get-Process -Id ([int]$record.owner_pid) -ErrorAction SilentlyContinue
        if($owner){
            if($owner.StartTime.ToUniversalTime().ToString('o') -ne ([datetime]$record.owner_start).ToUniversalTime().ToString('o')){throw 'Legacy ownership identity does not match; no process was stopped.'}
            $prefix=$root.TrimEnd('\','/')+[IO.Path]::DirectorySeparatorChar
            $children=@(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($owner.Id)"|Where-Object{$_.ExecutablePath -and $_.ExecutablePath.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)})
            foreach($child in $children){
                $process=Get-Process -Id $child.ProcessId -ErrorAction SilentlyContinue
                if($process -and $process.StartTime -ge $owner.StartTime){$process.Kill($true);if(-not $process.WaitForExit(5000)){throw 'Owned legacy child termination could not be confirmed.'}}
            }
        }
    }
    if([IO.File]::Exists($state)){[IO.File]::Delete($state)}
    Write-RunLog INFO cleanup 'Owned server stopped or already exited.'
}catch{$code=1;Write-RunLog ERROR cleanup $_.Exception.Message}
finally{Write-RunLog INFO cleanup "Stop finished exit_code=$code."}
exit $code
