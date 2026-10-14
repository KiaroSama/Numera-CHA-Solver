#requires -Version 7.0
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:Root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$script:Log = $null

function Assert-Hosted {
    if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
        throw 'Build, runtime acquisition and validation run only on GitHub-hosted CI.'
    }
}
function Get-HostedRoot { $script:Root }
function Get-HostedCommand([string]$Name) {
    try { $commands=@(Get-Command $Name -CommandType Application -All -ErrorAction Stop) }
    catch { throw "Hosted executable is unavailable: $Name." }
    if ($commands.Count -eq 0 -or -not [IO.File]::Exists($commands[0].Source)) { throw "Hosted executable is unavailable: $Name." }
    return $commands[0].Source
}
function Initialize-HostedLog([string]$Name) {
    Assert-Hosted
    try {
        $directory = Join-Path $script:Root 'logs'
        [IO.Directory]::CreateDirectory($directory) | Out-Null
        $stem = "$Name`_$([DateTime]::UtcNow.ToString('yyyy-MM-dd_HH-mm-ss'))_UTC"
        $script:Log = Join-Path $directory "$stem`_$([guid]::NewGuid().ToString('N').Substring(0,8)).log"
        [IO.File]::WriteAllText($script:Log, '', [Text.UTF8Encoding]::new($false))
    } catch { [Console]::Error.WriteLine('File logging unavailable; stderr fallback active.') }
    Write-HostedLog INFO startup 'Hosted operation started.'
}
function Write-HostedLog([string]$Level, [string]$Component, [string]$Message) {
    $safe = $Message -replace '(?i)Bearer\s+\S+', 'Bearer [REDACTED]'
    $safe = $safe -replace '(?i)(["'']?(?:token|secret|password|cookies?|authorization|api[_-]?key)["'']?\s*[:=]\s*)(?:"(?:\\.|[^"\\])*"|''(?:\\.|[^''\\])*''|[^\s,;}]+)', '$1[REDACTED]'
    $safe = $safe.Replace("`r",'\r').Replace("`n",'\n')
    $line = "[$([DateTime]::UtcNow.ToString('yyyy-MM-dd HH:mm:ss')) UTC] [$Level] [$Component] $safe"
    [Console]::Error.WriteLine($line)
    if ($script:Log) {
        try { [IO.File]::AppendAllText($script:Log, $line+[Environment]::NewLine, [Text.UTF8Encoding]::new($false)) }
        catch { $script:Log=$null; [Console]::Error.WriteLine('File logging unavailable; stderr fallback active.') }
    }
}
function Invoke-Hosted([string]$Executable, [string[]]$Arguments, [int]$Wall=120, [int]$Idle=45, [string]$Capture='', [string]$Cwd=$script:Root) {
    Assert-Hosted
    if (-not [IO.Path]::IsPathRooted($Executable)) { $Executable=Get-HostedCommand $Executable }
    if ($IsWindows) {
        Import-Module (Join-Path $script:Root 'scripts/Common.psm1') -Force
        Initialize-RunLog 'hosted-command'
        return (Invoke-BoundedCommand -Executable $Executable -Arguments $Arguments -WallSeconds $Wall -IdleSeconds $Idle -CapturePath $Capture -WorkingDirectory $Cwd)
    }
    $owner = Join-Path $PSScriptRoot 'run_owned.py'
    $python = Get-HostedCommand python
    $parameters = @('-I',$owner,'--cwd',$Cwd,'--work',(Join-Path $script:Root '.ci-work/owners'),'--wall',"$Wall",'--idle',"$Idle")
    if ($Capture) { $parameters += @('--capture',$Capture) }
    & $python @parameters -- $Executable @Arguments
    return $LASTEXITCODE
}
function Assert-Exit([int]$Code, [string]$Operation) {
    if ($Code -ne 0) { throw "Hosted stage failed: $Operation (exit_code=$Code)." }
}
function Assert-RegularFile([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Expected a regular non-linked file.' }
    for ($directory=$item.Directory; $directory; $directory=$directory.Parent) {
        if ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'File input traverses a linked directory.' }
    }
    return $item
}
Export-ModuleMember -Function Assert-Hosted,Get-HostedRoot,Get-HostedCommand,Initialize-HostedLog,Write-HostedLog,Invoke-Hosted,Assert-Exit,Assert-RegularFile
