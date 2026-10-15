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
function Get-EntryHash($Entry) {
    $stream=$Entry.Open();$hash=[Security.Cryptography.SHA256]::Create()
    try{return [Convert]::ToHexString($hash.ComputeHash($stream)).ToLowerInvariant()}
    finally{$hash.Dispose();$stream.Dispose()}
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
    $assets=[Collections.Generic.List[string]]::new();$hashes=[Collections.Generic.List[string]]::new()
    foreach($platform in $platforms){
        $folder=Join-Path $directory.FullName "numera-cha-$version-$platform"
        $checksums=Assert-RegularFile (Join-Path $folder 'SHA256SUMS.txt')
        $lines=@([IO.File]::ReadAllLines($checksums.FullName,[Text.Encoding]::UTF8))
        $expected=@("numera-cha-$version-$platform.zip","numera-cha-desktop-$version-$platform.zip")
        if($lines.Count -ne 2 -or @(Get-ChildItem -LiteralPath $folder -Force).Count -ne 3){
            throw 'Platform release inventory is incomplete or contains unexpected files.'
        }
        foreach($name in $expected){
            $file=Assert-RegularFile (Join-Path $folder $name)
            $digest=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            if(@($lines|Where-Object {$_ -ceq "$digest  $name"}).Count -ne 1){throw 'Release archive checksum differs.'}
            $zip=[IO.Compression.ZipFile]::OpenRead($file.FullName)
            try {
                $entries=@{};[long]$total=0
                foreach($entry in $zip.Entries){
                    $path=$entry.FullName
                    if($path -match '(^/|\\|(^|/)\.\.(/|$)|(^|/)(\.ai|\.specify|specs|\.claude|\.codex|\.cursor|\.kiro|\.agents|graphify-out|\.codebase-memory|plans|\.git|\.ignoreme|secrets\.md|explain-AI\.md|reference\.md|CLAUDE\.md|AGENTS\.md|\.env[^/]*|api-token\.txt|mcp-token\.txt|server\.json)(/|$)|\.(sqlite3|onnx)$' -or $entries.ContainsKey($path)){
                        throw 'Release archive contains an unsafe, duplicate or protected entry.'
                    }
                    $total+=$entry.Length
                    if($entry.Length -gt 512MB -or $total -gt 1GB){throw 'Release archive exceeds inspection limits.'}
                    $entries[$path]=$entry
                }
                if(-not $entries.ContainsKey('package-manifest.json')){throw 'Release manifest is missing.'}
                $reader=[IO.StreamReader]::new($entries['package-manifest.json'].Open(),[Text.Encoding]::UTF8)
                try{$package=$reader.ReadToEnd()|ConvertFrom-Json}finally{$reader.Dispose()}
                $kind=if($name -ceq "numera-cha-$version-$platform.zip"){'cli-agent'}else{'desktop-installer'}
                $signing=if($platform -ceq 'macos-arm64'){'ad-hoc; not notarized'}else{'unsigned'}
                if($package.version -cne $version -or $package.platform -cne $platform -or $package.source_commit -cne $sha -or $package.schema_version -ne 1 -or $package.kind -cne $kind -or $package.signing -cne $signing){
                    throw 'Release package identity differs from the verified source.'
                }
                if($package.files.Count -ne ($entries.Count-1)){throw 'Release manifest coverage differs.'}
                $seen=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
                foreach($item in $package.files){
                    if(-not $seen.Add($item.path) -or -not $entries.ContainsKey($item.path)) {throw 'Release manifest entry is missing or duplicated.'}
                    $entry=$entries[$item.path]
                    if($entry.Length -ne $item.size -or (Get-EntryHash $entry) -cne $item.sha256){throw 'Release manifest entry bytes differ.'}
                }
                foreach($required in @('LICENSE','THIRD_PARTY_NOTICES.md','licenses/dependencies.json',"source/numera-cha-$version-source-$sha.zip",'source/REVISION.txt')){
                    if(-not $entries.ContainsKey($required)){throw 'Release matching source or independent notices are missing.'}
                }
            } finally {$zip.Dispose()}
            $assets.Add($file.FullName);$hashes.Add("$digest  $name")
        }
    }
    $summary=Join-Path $directory.FullName 'SHA256SUMS.txt'
    [IO.File]::WriteAllLines($summary,$hashes,[Text.UTF8Encoding]::new($false));$assets.Add($summary)
    Write-HostedLog INFO verification 'All six native archives match exact source, file manifests, checksums and publication boundaries.'
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
            [IO.File]::WriteAllText($notes,"Native Rust accessibility workspace and authenticated MCP/bridge.`n`nSource: $sha`nValidation: https://github.com/$repository/actions/runs/$VerifiedRunId`n`nWindows/Linux packages are unsigned; macOS Apple Silicon is ad-hoc signed and not notarized. Native Windows/macOS GUI interaction, GPU and real CAPTCHA accuracy are not certified. Model weights and private user state are excluded. Read the included custom license and independent notices.`n",[Text.UTF8Encoding]::new($false))
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
