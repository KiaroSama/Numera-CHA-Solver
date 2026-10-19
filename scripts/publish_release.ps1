#requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Artifacts,[switch]$Publish,[string]$VerifiedRunId)
Import-Module (Join-Path $PSScriptRoot '../desktop/ci/Hosted.psm1') -Force
Initialize-HostedLog 'publish-release'
$code=0
function Invoke-GitHub([string[]]$Arguments) {
    $start=[Diagnostics.ProcessStartInfo]::new()
    $start.FileName=Get-HostedCommand gh
    foreach($argument in $Arguments){$start.ArgumentList.Add($argument)}
    $start.UseShellExecute=$false;$start.CreateNoWindow=$true
    $start.RedirectStandardInput=$true;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
    $process=[Diagnostics.Process]::new();$process.StartInfo=$start
    try {
        $null=$process.Start();$process.StandardInput.Close()
        $stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync()
        if(-not $process.WaitForExit(120000)){
            $process.Kill($true);$null=$process.WaitForExit(5000)
            throw 'GitHub release operation exceeded its deadline; inspect remote state before retrying.'
        }
        if($process.ExitCode -ne 0){throw 'GitHub release operation failed; no automatic retry or asset overwrite is allowed.'}
        return $stdout.GetAwaiter().GetResult()
    } finally {$process.Dispose()}
}
function Assert-ReleaseTag([string]$Repository,[string]$Tag,[string]$Sha) {
    $refs=@(Invoke-GitHub -Arguments @('api',"repos/$Repository/git/matching-refs/tags/$Tag")|ConvertFrom-Json|Where-Object {$_.ref -ceq "refs/tags/$Tag"})
    if($refs.Count -gt 1 -or ($refs.Count -eq 1 -and ($refs[0].object.type -cne 'commit' -or $refs[0].object.sha -cne $Sha))){
        throw 'Existing release tag differs or requires separate annotated-tag review; publication stopped.'
    }
}
try {
    $root=Get-HostedRoot
    $repository=$env:GITHUB_REPOSITORY;$sha=$env:GITHUB_SHA
    if($repository -cne 'KiaroSama/Numera-CHA-Solver' -or $sha -notmatch '^[a-f0-9]{40}$'){
        throw 'Release repository/source identity is unavailable or unexpected.'
    }
    $manifest=[IO.File]::ReadAllText((Join-Path $root 'Cargo.toml'),[Text.Encoding]::UTF8)
    $version=[regex]::Match($manifest,'(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"\s*$').Groups[1].Value
    if(-not $version){throw 'A stable project version is required.'}
    $directory=Get-Item -LiteralPath $Artifacts -Force
    if(-not $directory.FullName.StartsWith((Join-Path $root '.ci-work')+[IO.Path]::DirectorySeparatorChar,[StringComparison]::Ordinal)){
        throw 'Release inspection must stay in this hosted checkout work directory.'
    }
    if(-not $directory.PSIsContainer -or ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint)){
        throw 'Expected an ordinary release artifact directory.'
    }
    $platforms=@('windows-x64','linux-x64','macos-arm64')
    $validator=Join-Path $root '.ci-work/release-validator/distribution'
    if(-not [IO.File]::Exists($validator)){throw 'Exact-run native distribution validator is missing.'}
    if(-not $IsWindows){[IO.File]::SetUnixFileMode($validator,[IO.UnixFileMode]::UserRead -bor [IO.UnixFileMode]::UserWrite -bor [IO.UnixFileMode]::UserExecute)}
    $assets=[Collections.Generic.List[string]]::new()
    $info=[Diagnostics.ProcessStartInfo]::new();$info.FileName=$validator
    foreach($argument in @('verify','--artifacts',$directory.FullName,'--source',$sha,'--version',$version)){$info.ArgumentList.Add($argument)}
    $info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardInput=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
    $process=[Diagnostics.Process]::new();$process.StartInfo=$info
    try{
        $null=$process.Start();$process.StandardInput.Close();$out=$process.StandardOutput.ReadToEndAsync();$err=$process.StandardError.ReadToEndAsync()
        if(-not $process.WaitForExit(120000)){$process.Kill($true);$null=$process.WaitForExit(5000);throw 'Native release inspection timed out.'}
        if($process.ExitCode -ne 0){[Console]::Error.WriteLine($err.GetAwaiter().GetResult());throw 'Native release inspection failed.'}
        foreach($asset in ($out.GetAwaiter().GetResult() -split "`n")){if($asset.Trim()){$assets.Add($asset.Trim())}}
    }finally{$process.Dispose()}
    if($assets.Count -ne 13){throw 'Native verified release inventory differs.'}
    & (Join-Path $PSScriptRoot 'sign_release.ps1') -Phase VerifyArchives -Path $directory.FullName
    foreach($name in @('publisher-cert.pem','publisher-public.pem','publisher.json')){$assets.Add((Join-Path $root "resources/signing/$name"))}
    Write-HostedLog INFO verification 'Native manifests/source/archives and pinned cryptographic signatures verified.'
    if($Publish){
        if($env:GITHUB_REF -cne 'refs/heads/main' -or $env:GITHUB_EVENT_NAME -cne 'workflow_dispatch'){
            throw 'Release publication requires the verified manual default-branch batch.'
        }
        if($VerifiedRunId -notmatch '^[0-9]+$'){throw 'An exact successful validation run is required.'}
        $run=Invoke-GitHub -Arguments @('api',"repos/$repository/actions/runs/$VerifiedRunId")|ConvertFrom-Json
        if($run.head_sha -cne $sha -or $run.conclusion -cne 'success' -or $run.path -cne '.github/workflows/ci.yml' -or $run.repository.full_name -cne $repository){
            throw 'Validation run identity or conclusion differs from the release source.'
        }
        $jobs=(Invoke-GitHub -Arguments @('api',"repos/$repository/actions/runs/$VerifiedRunId/jobs?per_page=100")|ConvertFrom-Json).jobs
        foreach($requiredJob in (@($platforms|ForEach-Object {"Native / $_"})+'Verify release assets')){
            $job=@($jobs|Where-Object {$_.name -ceq $requiredJob})
            if($job.Count -ne 1 -or $job[0].conclusion -cne 'success'){throw 'Every required native and asset-verification job must pass before publication.'}
        }
        $branch=Invoke-GitHub -Arguments @('api',"repos/$repository/git/ref/heads/main")|ConvertFrom-Json
        if($branch.object.sha -cne $sha){throw 'Default branch moved; release promotion requires reinspection.'}
        $tag="v$version"
        Assert-ReleaseTag $repository $tag $sha
        $releases=@(Invoke-GitHub -Arguments @('api',"repos/$repository/releases?per_page=100")|ConvertFrom-Json)
        if(@($releases|Where-Object {$_.tag_name -ceq $tag}).Count){
            throw 'This version already has a release; inspect it before selecting a new version. Existing assets were not changed.'
        } else {
            $notes=Join-Path $directory.FullName 'release-notes.md'
            [IO.File]::WriteAllText($notes,"Native Rust accessibility workspace and authenticated MCP/bridge.`n`nSource: $sha`nValidation: https://github.com/$repository/actions/runs/$VerifiedRunId`n`nOwner self-signed release: Windows native Authenticode and macOS code signatures, plus detached RSA/SHA256 archive signatures. Not publicly trusted or notarized; OS warnings may remain. Native Windows/macOS GUI interaction, GPU and real CAPTCHA accuracy are not certified. Model weights and private user state are excluded. Read the included custom license and independent notices.`n",[Text.UTF8Encoding]::new($false))
            $null=Invoke-GitHub -Arguments (@('release','create',$tag,'--repo',$repository,'--target',$sha,'--draft','--title',"Numera CHA-Solver $version",'--notes-file',$notes)+$assets.ToArray())
            $release=Invoke-GitHub -Arguments @('api',"repos/$repository/releases?per_page=100")|ConvertFrom-Json|Where-Object {$_.tag_name -ceq $tag}
            if(@($release).Count -ne 1 -or -not $release.draft -or $release.assets.Count -ne $assets.Count){throw 'Draft release inventory differs; publication stopped.'}
            foreach($asset in $release.assets){
                $local=@($assets|Where-Object {[IO.Path]::GetFileName($_) -ceq $asset.name})
                if($local.Count -ne 1 -or $asset.digest -cne ('sha256:'+(Get-FileHash -LiteralPath $local[0] -Algorithm SHA256).Hash.ToLowerInvariant())){throw 'Remote draft asset digest differs; publication stopped.'}
            }
            if($release.target_commitish -cne $sha){throw 'Draft release target differs from the verified source.'}
            Assert-ReleaseTag $repository $tag $sha
            $null=Invoke-GitHub -Arguments @('release','edit',$tag,'--repo',$repository,'--draft=false','--latest')
            $published=Invoke-GitHub -Arguments @('api',"repos/$repository/releases/$($release.id)")|ConvertFrom-Json
            $ref=Invoke-GitHub -Arguments @('api',"repos/$repository/git/ref/tags/$tag")|ConvertFrom-Json
            if($published.draft -or $ref.object.sha -cne $sha){throw 'Published release/source readback differs.'}
            Write-HostedLog INFO release "Published verified $tag; local inspection copies belong to disposable hosted work."
        }
    }
} catch {$code=1;Write-HostedLog ERROR release $_.Exception.Message}
finally {Write-HostedLog INFO cleanup "Release promotion finished exit_code=$code."}
exit $code
