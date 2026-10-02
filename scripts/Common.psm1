#requires -Version 7.0
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:Root = Split-Path -Parent $PSScriptRoot
$script:LogPath = $null

function Initialize-RunLog([string]$Name) {
    $directory = Join-Path $script:Root 'logs'
    try {
        [IO.Directory]::CreateDirectory($directory) | Out-Null
        $stamp = [DateTime]::UtcNow.ToString('yyyy-MM-dd_HH-mm-ss')
        $path = Join-Path $directory "$($Name)_$($stamp)_UTC.log"
        if (Test-Path -LiteralPath $path) { $path = Join-Path $directory "$($Name)_$($stamp)_UTC_$([guid]::NewGuid().ToString('N').Substring(0,8)).log" }
        $script:LogPath = $path
        [IO.File]::WriteAllText($path, '', [Text.UTF8Encoding]::new($false))
        Write-RunLog INFO startup 'Diagnostic logging initialized.'
    } catch {
        $script:LogPath = $null
        [Console]::Error.WriteLine('File logging unavailable; stderr fallback active.')
    }
}

function Write-RunLog([ValidateSet('INFO','WARNING','ERROR','DEBUG')][string]$Level, [string]$Component, [string]$Message) {
    $safe = $Message -replace '(?i)(token|secret|password|cookie|authorization|api[_-]?key)\s*[:=]\s*\S+', '$1=[REDACTED]' -replace '(?i)Bearer\s+\S+', 'Bearer [REDACTED]'
    $line = "[$([DateTime]::UtcNow.ToString('yyyy-MM-dd HH:mm:ss')) UTC] [$Level] [$Component] $safe"
    [Console]::Error.WriteLine($line)
    if ($script:LogPath) {
        try { [IO.File]::AppendAllText($script:LogPath, $line+[Environment]::NewLine, [Text.UTF8Encoding]::new($false)) }
        catch { [Console]::Error.WriteLine('Writing diagnostic log failed.') }
    }
}

function Get-ProjectRoot { return $script:Root }
function Get-ProjectPython {
    $python = Join-Path $script:Root '.venv\Scripts\python.exe'
    if (-not (Test-Path -LiteralPath $python -PathType Leaf)) { throw 'Project environment is missing. Run scripts/setup.ps1 first.' }
    return $python
}

function Invoke-BoundedCommand {
    param([Parameter(Mandatory)][string]$Executable, [string[]]$Arguments=@(), [int]$WallSeconds=300, [int]$IdleSeconds=60, [string]$WorkingDirectory=$script:Root)
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Executable
    foreach($argument in $Arguments) { $info.ArgumentList.Add($argument) }
    $info.WorkingDirectory = $WorkingDirectory
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $info.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    $temporary = Join-Path $script:Root '.ci-cache\tmp'
    [IO.Directory]::CreateDirectory($temporary) | Out-Null
    $info.Environment['TEMP'] = $temporary
    $info.Environment['TMP'] = $temporary
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $info
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $lastProgress = 0.0
    $lastCpu = 0.0
    $outputFile=$null; $errorFile=$null; $outputPath=$null; $errorPath=$null
    try {
        $process.Start() | Out-Null
        $process.StandardInput.Close()
        Write-RunLog INFO process "Owned process started pid=$($process.Id)."
        $outputPath = Join-Path $temporary ([guid]::NewGuid().ToString('N')+'.out')
        $errorPath = Join-Path $temporary ([guid]::NewGuid().ToString('N')+'.err')
        $outputFile = [IO.File]::Create($outputPath)
        $errorFile = [IO.File]::Create($errorPath)
        $output = $process.StandardOutput.BaseStream.CopyToAsync($outputFile)
        $errorOutput = $process.StandardError.BaseStream.CopyToAsync($errorFile)
        $lastBytes = 0L
        while (-not $process.WaitForExit(500)) {
            $cpu = $process.TotalProcessorTime.TotalSeconds
            $bytes = $outputFile.Position+$errorFile.Position
            if ($cpu -gt $lastCpu+0.1 -or $bytes -gt $lastBytes) { $lastProgress=$watch.Elapsed.TotalSeconds; $lastCpu=$cpu; $lastBytes=$bytes }
            if ($watch.Elapsed.TotalSeconds -gt $WallSeconds -or $watch.Elapsed.TotalSeconds-$lastProgress -gt $IdleSeconds) {
                $process.Kill($true)
                $process.WaitForExit(5000) | Out-Null
                throw 'Owned process exceeded its wall or progress ceiling.'
            }
        }
        if (-not $output.Wait(5000) -or -not $errorOutput.Wait(5000)) { throw 'Child output handles did not close.' }
        $outputFile.Flush(); $outputFile.Dispose(); $outputFile=$null
        $errorFile.Flush(); $errorFile.Dispose(); $errorFile=$null
        [Console]::Out.Write([IO.File]::ReadAllText($outputPath,[Text.Encoding]::UTF8))
        [Console]::Error.Write([IO.File]::ReadAllText($errorPath,[Text.Encoding]::UTF8))
        Write-RunLog INFO process "Owned process completed exit_code=$($process.ExitCode)."
        return $process.ExitCode
    } finally {
        try { if (-not $process.HasExited) { $process.Kill($true); $process.WaitForExit(5000) | Out-Null } } catch { }
        if ($outputFile) { $outputFile.Dispose() }
        if ($errorFile) { $errorFile.Dispose() }
        foreach($path in @($outputPath,$errorPath)) { if($path -and (Test-Path -LiteralPath $path)){ Remove-Item -LiteralPath $path } }
        $process.Dispose()
    }
}
Export-ModuleMember -Function Initialize-RunLog,Write-RunLog,Get-ProjectRoot,Get-ProjectPython,Invoke-BoundedCommand
