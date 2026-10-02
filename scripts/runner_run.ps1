#requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$')][string]$Repository,[string]$Branch='main')
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'runner-run'
$process=$null
$code=0
try {
    $root=Get-ProjectRoot
    $directory=Join-Path $root '.ci-runner-win'
    $listener=Join-Path $directory 'bin\Runner.Listener.exe'
    if (-not (Test-Path -LiteralPath (Join-Path $directory '.runner'))) { throw 'Dedicated project runner is not registered.' }
    $gh=(Get-Command gh -ErrorAction Stop).Source
    $sha=& git rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Local commit is unavailable.' }
    $remote=& $gh api "repos/$Repository/commits/$Branch" --jq .sha
    if ($LASTEXITCODE -ne 0 -or $remote -ne $sha) { throw 'Remote branch does not match the exact local commit.' }
    $info=[Diagnostics.ProcessStartInfo]::new()
    $info.FileName=$listener
    $info.ArgumentList.Add('run');$info.ArgumentList.Add('--once')
    $info.WorkingDirectory=$directory
    $info.UseShellExecute=$false;$info.CreateNoWindow=$true
    $info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$info.RedirectStandardInput=$true
    $process=[Diagnostics.Process]::new();$process.StartInfo=$info
    $process.Start()|Out-Null;$process.StandardInput.Close()
    $out=$process.StandardOutput.ReadToEndAsync();$err=$process.StandardError.ReadToEndAsync()
    Write-RunLog INFO runner "Owned runner started pid=$($process.Id)."
    & $gh workflow run ci.yml --repo $Repository --ref $Branch
    if ($LASTEXITCODE -ne 0) { throw 'Manual workflow dispatch failed.' }
    $watch=[Diagnostics.Stopwatch]::StartNew()
    while (-not $process.WaitForExit(2000)) {
        if ($watch.Elapsed.TotalSeconds -gt 900) { throw 'Manual runner exceeded its lifecycle ceiling.' }
    }
    $code=$process.ExitCode
    Write-RunLog INFO runner "Owned single-job runner exited exit_code=$code."
    $runs=& $gh run list --repo $Repository --workflow ci.yml --commit $sha --limit 5 --json databaseId,status,conclusion,headSha,url | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cannot verify exact-SHA workflow outcome.' }
    $run=$runs|Where-Object {$_.headSha -eq $sha}|Select-Object -First 1
    if (-not $run -or $run.status -ne 'completed' -or $run.conclusion -ne 'success') {
        if($run){Write-Output ($run|ConvertTo-Json -Compress)}
        throw 'Exact-SHA CI did not complete successfully.'
    }
    Write-Output ($run|ConvertTo-Json -Compress)
} catch { $code=1; Write-RunLog ERROR runner $_.Exception.Message }
finally {
    if ($process) {
        if (-not $process.HasExited) { $process.Kill($true);$process.WaitForExit(5000)|Out-Null }
        Write-RunLog INFO cleanup "Owned runner stopped pid=$($process.Id)."
        $process.Dispose()
    }
}
exit $code
