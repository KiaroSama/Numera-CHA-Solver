#requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$')][string]$Repository)
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'runner-setup'
$code=0
try {
    $root=Get-ProjectRoot
    $directory=Join-Path $root '.ci-runner-win'
    if (Test-Path -LiteralPath (Join-Path $directory '.runner')) { Write-RunLog INFO runner 'Project runner is already registered; no changes made.'; exit 0 }
    [IO.Directory]::CreateDirectory($directory) | Out-Null
    $gh=(Get-Command gh -ErrorAction Stop).Source
    $release=& $gh api repos/actions/runner/releases/latest | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read official runner release.' }
    $asset=$release.assets | Where-Object name -Like 'actions-runner-win-x64-*.zip' | Select-Object -First 1
    if (-not $asset -or $asset.browser_download_url -notlike 'https://github.com/actions/runner/releases/download/*' -or $asset.digest -notmatch '^sha256:[a-f0-9]{64}$') { throw 'Runner release identity/checksum is unavailable.' }
    $archive=Join-Path $directory 'runner.zip'
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $archive -TimeoutSec 120
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $asset.digest.Substring(7)) { throw 'Runner archive checksum mismatch.' }
    Expand-Archive -LiteralPath $archive -DestinationPath $directory
    Remove-Item -LiteralPath $archive
    $registration=& $gh api --method POST "repos/$Repository/actions/runners/registration-token" | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or -not $registration.token) { throw 'Repository runner registration permission is unavailable.' }
    $env:ACTIONS_RUNNER_INPUT_TOKEN=$registration.token
    try {
        $work=Join-Path $root '.ci-work\windows'
        $code=Invoke-BoundedCommand -Executable (Join-Path $directory 'bin\Runner.Listener.exe') -Arguments @('configure','--unattended','--url',"https://github.com/$Repository",'--name','numera-cha-windows','--labels','numera-cha,gpu','--work',$work) -WorkingDirectory $directory -WallSeconds 120 -IdleSeconds 60
        if ($code -ne 0) { throw 'Runner configuration failed.' }
    } finally { Remove-Item Env:ACTIONS_RUNNER_INPUT_TOKEN -ErrorAction SilentlyContinue; $registration=$null }
    Write-RunLog INFO runner 'Dedicated repository runner registered, offline, with no service or startup entry.'
} catch { $code=1; Write-RunLog ERROR runner 'Runner setup failed; inspect the captured safe diagnostic output.' }
finally { Write-RunLog INFO cleanup "Runner setup finished exit_code=$code." }
exit $code
