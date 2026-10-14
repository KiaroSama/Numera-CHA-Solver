#requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][ValidateSet('windows-x64','linux-x64','macos-arm64')][string]$Platform)
Import-Module (Join-Path $PSScriptRoot '../desktop/ci/Hosted.psm1') -Force
Initialize-HostedLog 'prepare-runtime'
$code=0
try {
    $root=Get-HostedRoot
    $runtime=Join-Path $root '.ci-work/runtime'
    $work=Join-Path $root ('.ci-work/downloads/'+[guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($work)|Out-Null
    $release=@{
        'windows-x64'=@{name='onnxruntime-win-x64-1.31.0.zip';hash='3958d8a44984160692c1b4ffd0de3cb48f8c39f9d34bb31e433db06e30b82195';library='onnxruntime.dll'}
        'linux-x64'=@{name='onnxruntime-linux-x64-1.31.0.tgz';hash='cc5c72baf5ae5c8238a6841f0897227be2d02826b9cf98eaf02fdefaeeb03a57';library='libonnxruntime.so'}
        'macos-arm64'=@{name='onnxruntime-osx-arm64-1.31.0.tgz';hash='1031b36dd3bdaa8d976a6130d9cf2a05e9be320b93ae804f8427b91dc3432afc';library='libonnxruntime.dylib'}
    }[$Platform]
    $archive=Join-Path $work $release.name
    Write-HostedLog INFO download "Acquiring official CPU runtime platform=$Platform version=1.31.0."
    Invoke-WebRequest -Uri "https://github.com/microsoft/onnxruntime/releases/download/v1.31.0/$($release.name)" -OutFile $archive -ConnectionTimeoutSeconds 30 -OperationTimeoutSeconds 180 -MaximumRedirection 5
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $release.hash) { throw 'Official runtime archive SHA-256 differs from the release pin.' }
    $unpacked=Join-Path $work 'unpacked';[IO.Directory]::CreateDirectory($unpacked)|Out-Null
    if ($Platform -eq 'windows-x64') {
        $zip=[IO.Compression.ZipFile]::OpenRead($archive)
        try {
            foreach ($entry in $zip.Entries) {
                if ($entry.FullName -match '(^/|^[A-Za-z]:|(^|[\\/])\.\.([\\/]|$))' -or (($entry.ExternalAttributes -shr 16) -band 0xf000) -eq 0xa000) { throw 'Runtime archive contains an unsafe entry.' }
            }
        } finally { $zip.Dispose() }
        [IO.Compression.ZipFile]::ExtractToDirectory($archive,$unpacked)
    } else {
        $inventory=Join-Path $work 'entries.txt'
        Assert-Exit (Invoke-Hosted tar @('-tzf',$archive) 30 15 $inventory) 'runtime-archive-inventory'
        foreach ($entry in [IO.File]::ReadAllLines($inventory,[Text.Encoding]::UTF8)) {
            if ($entry -match '(^/|(^|/)\.\.(/|$))') { throw 'Runtime archive contains an unsafe path.' }
        }
        Assert-Exit (Invoke-Hosted tar @('-xzf',$archive,'-C',$unpacked) 45 20) 'runtime-extraction'
    }
    $extracted=@(Get-ChildItem -LiteralPath $unpacked -Directory)
    if ($extracted.Count -ne 1) { throw 'Official runtime layout differs from the reviewed single root.' }
    $lib=Join-Path $extracted[0].FullName 'lib'
    $destination=Join-Path $runtime 'data/runtime'
    [IO.Directory]::CreateDirectory((Join-Path $destination 'licenses'))|Out-Null
    foreach ($file in (Get-ChildItem -LiteralPath $lib -File)) {
        if ($file.Name -notmatch '(\.dll$|\.so($|\.)|\.dylib$)') { continue }
        $source=$file.FullName
        if ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) {
            $resolved=$file.ResolveLinkTarget($true)
            if (-not $resolved -or $resolved.FullName -notlike "$lib$([IO.Path]::DirectorySeparatorChar)*") { throw 'Runtime library alias escapes the verified library directory.' }
            $source=$resolved.FullName;$null=Assert-RegularFile $source
        }
        [IO.File]::Copy($source,(Join-Path $destination $file.Name),$false)
    }
    $canonical=Join-Path $destination $release.library
    if (-not [IO.File]::Exists($canonical)) {
        $versioned=if ($Platform -eq 'linux-x64') {'libonnxruntime.so.1.31.0'} else {'libonnxruntime.1.31.0.dylib'}
        $source=Join-Path $destination $versioned; $null=Assert-RegularFile $source
        [IO.File]::Copy($source,$canonical,$false)
    }
    if ($Platform -eq 'macos-arm64') {
        # Materialized runtime aliases are independently ad-hoc signed before inclusion as bundle resources.
        foreach ($file in (Get-ChildItem -LiteralPath $destination -File -Filter '*.dylib')) {
            Assert-Exit (Invoke-Hosted codesign @('--force','--sign','-',$file.FullName) 30 15) 'runtime-ad-hoc-signing'
        }
    }
    foreach ($name in @('LICENSE','ThirdPartyNotices.txt')) {
        $source=Join-Path $extracted[0].FullName $name;$null=Assert-RegularFile $source
        [IO.File]::Copy($source,(Join-Path $destination "licenses/$name"),$false)
    }
    $manifest=[IO.File]::ReadAllText((Join-Path $root 'resources/models.json'),[Text.Encoding]::UTF8)|ConvertFrom-Json
    $model=@($manifest.models|Where-Object {$_.id -eq 'ppocr-v6-small'})
    if ($model.Count -ne 1 -or $model[0].revision -notmatch '^[0-9a-f]{40}$') { throw 'Pinned model manifest is invalid.' }
    $modelDirectory=Join-Path $runtime "data/models/$($model[0].id)/$($model[0].revision)"
    [IO.Directory]::CreateDirectory($modelDirectory)|Out-Null
    foreach ($asset in $model[0].assets) {
        $uri=[Uri]$asset.url
        if ($uri.Scheme -ne 'https' -or $uri.Host -notin @('huggingface.co','raw.githubusercontent.com') -or $asset.file -notmatch '^[A-Za-z0-9_.-]+$' -or $asset.sha256 -notmatch '^[a-f0-9]{64}$' -or $asset.size -le 0 -or $asset.size -gt 50000000) { throw 'Model acquisition manifest violates its reviewed boundary.' }
        $download=Join-Path $work $asset.file
        Invoke-WebRequest -Uri $uri -OutFile $download -ConnectionTimeoutSeconds 30 -OperationTimeoutSeconds 120 -MaximumRedirection 5
        if ((Get-Item -LiteralPath $download).Length -ne $asset.size -or (Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash.ToLowerInvariant() -ne $asset.sha256) { throw 'Immutable model length or SHA-256 verification failed.' }
        [IO.File]::Copy($download,(Join-Path $modelDirectory $asset.file),$false)
    }
    $desktop=Join-Path $root 'desktop/generated/resources'
    [IO.Directory]::CreateDirectory((Join-Path $desktop 'data'))|Out-Null
    Copy-Item -LiteralPath $destination -Destination (Join-Path $desktop 'data/runtime') -Recurse
    [IO.Directory]::CreateDirectory((Join-Path $desktop 'resources'))|Out-Null
    foreach ($name in @('models.json','capabilities.json')) { [IO.File]::Copy((Join-Path $root "resources/$name"),(Join-Path $desktop "resources/$name"),$false) }
    foreach ($name in @('LICENSE','THIRD_PARTY_NOTICES.md')) { [IO.File]::Copy((Join-Path $root $name),(Join-Path $desktop $name),$false) }
    $provenance=@{version='1.31.0';platform=$Platform;archive=$release.name;sha256=$release.hash;provider='CPUExecutionProvider';model_revision=$model[0].revision;post_extraction_signing=if($IsMacOS){'ad-hoc runtime aliases'}else{'unchanged archive binaries'}}
    [IO.File]::WriteAllText((Join-Path $destination 'runtime-manifest.json'),($provenance|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
    [IO.File]::Copy((Join-Path $destination 'runtime-manifest.json'),(Join-Path $desktop 'data/runtime/runtime-manifest.json'),$false)
    "NUMERA_RUNTIME_ROOT=$runtime"|Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
    "NUMERA_ORT_LIBRARY=$canonical"|Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
    Write-HostedLog INFO verification 'CPU runtime and real pinned model verified; only runtime resources staged for delivery.'
} catch { $code=1;Write-HostedLog ERROR runtime $_.Exception.Message }
finally { Write-HostedLog INFO cleanup "Runtime preparation finished exit_code=$code." }
exit $code
