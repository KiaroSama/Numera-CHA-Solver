#requires -Version 7.0
Import-Module (Join-Path $PSScriptRoot 'Hosted.psm1') -Force
Initialize-HostedLog 'hosted-contracts'
Import-Module (Join-Path $PSScriptRoot '../../scripts/DependencyNotices.psm1') -Force
$root=Get-HostedRoot
$work=Join-Path $root ('.ci-work/hosted-contracts/'+[guid]::NewGuid().ToString('N'))
$savedPath=$env:PATH
$code=0
function Expect-Failure([scriptblock]$Action,[string]$Message) {
    $failed=$false
    try { & $Action|Out-Null } catch { $failed=$_.Exception.Message.Contains($Message) }
    if (-not $failed) { throw 'Expected contract rejection was not observed.' }
}
try {
    $first=Join-Path $work 'first path';$second=Join-Path $work 'second path'
    $name='numera-command-'+[guid]::NewGuid().ToString('N')
    $filename=$name+$(if($IsWindows){'.exe'}else{''})
    foreach ($directory in @($first,$second)) {
        [IO.Directory]::CreateDirectory($directory)|Out-Null
        $path=Join-Path $directory $filename
        # Discovery only; neither placeholder is ever executed.
        [IO.File]::WriteAllText($path,"#!/bin/sh`nexit 1`n",[Text.UTF8Encoding]::new($false))
        if (-not $IsWindows) { [IO.File]::SetUnixFileMode($path,[IO.UnixFileMode]::UserRead -bor [IO.UnixFileMode]::UserWrite -bor [IO.UnixFileMode]::UserExecute) }
    }
    $env:PATH=$first+[IO.Path]::PathSeparator+$second+[IO.Path]::PathSeparator+$savedPath
    $matches=@(Get-Command $name -CommandType Application -All -ErrorAction Stop)
    if ($matches.Count -lt 2) { throw 'Duplicate PATH fixture did not exercise multiple commands.' }
    $selected=Get-HostedCommand $name
    if ($selected -isnot [string] -or $selected -cne (Join-Path $first $filename)) { throw 'Hosted discovery did not select exactly the first executable path.' }
    Expect-Failure {Get-HostedCommand ($name+'-missing')} 'Hosted executable is unavailable'
    $env:PATH=$savedPath

    $crate=Join-Path $work 'crate'
    [IO.Directory]::CreateDirectory($crate)|Out-Null
    $vcs=Join-Path $crate '.cargo_vcs_info.json'
    [IO.File]::WriteAllText($vcs,'{"git":{"sha1":"0a81fd6928ea3b33c8cd484aa4575d50ffb98012"}}',[Text.UTF8Encoding]::new($false))
    $dependency=[pscustomobject]@{name='alloc-stdlib';version='0.3.0';license='BSD-3-Clause';manifest_path=(Join-Path $crate 'Cargo.toml')}
    $fallback=Get-DependencyFallback $dependency $root
    if ($fallback.files.Count -ne 1 -or $fallback.selected_license -cne 'BSD-3-Clause') { throw 'Complete reviewed dependency grant was not selected.' }
    $dependency.version='0.3.1'
    Expect-Failure {Get-DependencyFallback $dependency $root} 'unavailable'
    $dependency.version='0.3.0';$dependency.license='MIT'
    Expect-Failure {Get-DependencyFallback $dependency $root} 'license expression differs'
    $dependency.license='BSD-3-Clause'
    [IO.File]::WriteAllText($vcs,'{"git":{"sha1":"0000000000000000000000000000000000000000"}}',[Text.UTF8Encoding]::new($false))
    Expect-Failure {Get-DependencyFallback $dependency $root} 'source revision differs'
    [IO.File]::WriteAllText($vcs,'{"git":{"sha1":"0a81fd6928ea3b33c8cd484aa4575d50ffb98012"}}',[Text.UTF8Encoding]::new($false))
    $fixture=Join-Path $work 'resources/licenses/upstream'
    [IO.Directory]::CreateDirectory($fixture)|Out-Null
    [IO.File]::WriteAllText((Join-Path $fixture 'alloc-BSD-3-Clause.txt'),'altered notice',[Text.UTF8Encoding]::new($false))
    Expect-Failure {Get-DependencyFallback $dependency $work} 'notice bytes differ'
    $portable=[IO.File]::ReadAllText((Join-Path $root 'plugin.json'),[Text.Encoding]::UTF8)|ConvertFrom-Json
    if($portable.'$schema' -cne 'https://agent-plugins.org/schemas/1.0.0/plugin.schema.json' -or $portable.name -cne 'numera-cha-solver' -or $portable.version -cne '0.4.2'){throw 'Portable plugin identity/schema differs.'}
    $allowed=@('$schema','name','version','description','author','homepage','repository','license','keywords','extensions')
    if(@($portable.PSObject.Properties.Name|Where-Object {$_ -cnotin $allowed}).Count){throw 'Portable plugin contains an unsupported field.'}
    foreach($relative in @('.claude-plugin/plugin.json','.codex-plugin/plugin.json','.cursor-plugin/plugin.json')){
        $native=[IO.File]::ReadAllText((Join-Path $root $relative),[Text.Encoding]::UTF8)|ConvertFrom-Json
        if($native.name -cne $portable.name -or $native.version -cne $portable.version -or $native.skills -cne './skills/'){throw 'Native plugin payload differs.'}
    }
    $null=Assert-RegularFile (Join-Path $root 'skills/numera-cha-solver/SKILL.md')
    $null=Assert-RegularFile (Join-Path $root 'run.ps1')
    Write-HostedLog INFO verification 'Duplicate PATH, fail-closed notices and original plugin structure contracts passed.'
} catch { $code=1;Write-HostedLog ERROR verification $_.Exception.Message }
finally {
    $env:PATH=$savedPath
    if ([IO.Directory]::Exists($work)) { Remove-Item -LiteralPath $work -Recurse -Force }
    Write-HostedLog INFO cleanup "Contract fixtures removed exit_code=$code."
}
exit $code
