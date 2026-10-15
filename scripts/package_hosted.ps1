#requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Resources','Archives')][string]$Phase,
    [Parameter(Mandatory)][ValidateSet('windows-x64','linux-x64','macos-arm64')][string]$Platform,
    [Parameter(Mandatory)][string]$Target
)
Import-Module (Join-Path $PSScriptRoot '../desktop/ci/Hosted.psm1') -Force
Initialize-HostedLog 'package-hosted'
$code=0
try {
    $root=Get-HostedRoot;$suffix=if ($IsWindows) {'.exe'} else {''}
    $resources=Join-Path $root 'desktop/generated/resources'
    $output=Join-Path $root '.ci-work/artifacts'
    [IO.Directory]::CreateDirectory($output)|Out-Null
    $release=Join-Path $root 'target/release'
    $runtime=Join-Path $env:NUMERA_RUNTIME_ROOT 'data/runtime'
    $sha=(& git -C $root rev-parse HEAD).Trim();if ($LASTEXITCODE -ne 0 -or $sha -notmatch '^[a-f0-9]{40}$') { throw 'Exact source revision is unavailable.' }
    $protected='(^|/)(\.ai|\.specify|specs|\.claude|\.codex|\.cursor|\.kiro|\.agents|graphify-out|\.codebase-memory|plans|\.git|data|logs|target|node_modules|\.ci[^/]*|\.ignoreme|secrets\.md|explain-AI\.md|reference\.md|CLAUDE\.md|AGENTS\.md|\.env[^/]*|server\.json|.*\.sqlite3)(/|$)'
    function Add-File([string]$Source,[string]$Destination) {
        $item=Assert-RegularFile $Source
        [IO.Directory]::CreateDirectory((Split-Path -Parent $Destination))|Out-Null
        [IO.File]::Copy($item.FullName,$Destination,$false)
    }
    function Write-Manifest([string]$Directory,[string]$Kind) {
        $files=@(Get-ChildItem -LiteralPath $Directory -File -Recurse -Force|ForEach-Object {
            $null=Assert-RegularFile $_.FullName
            $relative=[IO.Path]::GetRelativePath($Directory,$_.FullName).Replace('\','/')
            if ($relative -match '(^|/)(\.ai|\.specify|specs|\.claude|\.codex|\.cursor|\.kiro|\.ignoreme|secrets\.md|explain-AI\.md|\.env[^/]*|api-token\.txt|mcp-token\.txt|server\.json)(/|$)' -or $relative -match '(\.sqlite3|\.onnx)$') { throw 'Distribution includes a protected or user-data entry.' }
            @{path=$relative;size=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
        })
        $manifest=@{schema_version=1;version='0.3.0';platform=$Platform;kind=$Kind;source_commit=$sha;signing=if($Platform -eq 'macos-arm64'){'ad-hoc; not notarized'}else{'unsigned'};runtime='ONNX Runtime 1.31.0 CPU';files=$files}
        [IO.File]::WriteAllText((Join-Path $Directory 'package-manifest.json'),($manifest|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false))
    }
    function New-CheckedZip([string]$Directory,[string]$Path) {
        if ([IO.File]::Exists($Path)) { throw 'Artifact exists; a clean job output is required.' }
        $zip=[IO.Compression.ZipFile]::Open($Path,[IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($file in (Get-ChildItem -LiteralPath $Directory -File -Recurse -Force)) {
                $null=Assert-RegularFile $file.FullName
                $relative=[IO.Path]::GetRelativePath($Directory,$file.FullName).Replace('\','/')
                $entry=[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,$file.FullName,$relative,[IO.Compression.CompressionLevel]::Optimal)
                if (-not $IsWindows -and ($relative -match '^bin/' -or $relative -match '\.app/Contents/MacOS/')) { $entry.ExternalAttributes=(0x81ed -shl 16) }
            }
        } finally { $zip.Dispose() }
        $zip=[IO.Compression.ZipFile]::OpenRead($Path)
        try {
            $expected=@(Get-ChildItem -LiteralPath $Directory -File -Recurse -Force)
            if ($zip.Entries.Count -ne $expected.Count) { throw 'Archive count differs from the inspected staging allowlist.' }
            foreach ($entry in $zip.Entries) {
                $original=Join-Path $Directory $entry.FullName;$null=Assert-RegularFile $original
                $stream=$entry.Open();$hash=[Security.Cryptography.SHA256]::Create()
                try { $actual=[Convert]::ToHexString($hash.ComputeHash($stream)) } finally { $hash.Dispose();$stream.Dispose() }
                if ($entry.Length -ne (Get-Item -LiteralPath $original).Length -or $actual -ne (Get-FileHash -LiteralPath $original -Algorithm SHA256).Hash) { throw 'Archive bytes differ from the inspected manifest.' }
            }
        } finally { $zip.Dispose() }
    }
    if ($Phase -eq 'Resources') {
        $sidecar=Join-Path $root "desktop/binaries/numera-cha-$Target$suffix"
        Add-File (Join-Path $release "numera-cha$suffix") $sidecar
        $runtimeStaged=Join-Path $resources 'data/runtime'
        if (-not [IO.File]::Exists((Join-Path $runtimeStaged $(if($IsWindows){'onnxruntime.dll'}elseif($IsMacOS){'libonnxruntime.dylib'}else{'libonnxruntime.so'})))) {
            throw 'Checked CPU runtime has not been staged for the desktop bundle.'
        }
        foreach ($name in @('LICENSE','ThirdPartyNotices.txt')) { $null=Assert-RegularFile (Join-Path $runtimeStaged "licenses/$name") }
        $metadata=Join-Path $root '.ci-work/cargo-metadata.json'
        Assert-Exit (Invoke-Hosted cargo @('metadata','--locked','--offline','--format-version','1','--filter-platform',$Target) 60 30 $metadata) 'dependency-metadata'
        $document=[IO.File]::ReadAllText($metadata,[Text.Encoding]::UTF8)|ConvertFrom-Json
        $resolved=@($document.resolve.nodes|ForEach-Object {$_.id});$notices=[Collections.Generic.List[object]]::new()
        Import-Module (Join-Path $PSScriptRoot 'DependencyNotices.psm1') -Force
        foreach ($dependency in $document.packages) {
            if ($dependency.id -notin $resolved -or $dependency.source -eq $null) { continue }
            $directory=Split-Path -Parent $dependency.manifest_path
            $files=@(Get-ChildItem -LiteralPath $directory -File -Force|Where-Object {$_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE|UNLICENSE)([._-].*)?$'})
            if ($dependency.license_file) {
                $declared=[IO.Path]::GetFullPath((Join-Path $directory $dependency.license_file))
                if (-not $declared.StartsWith($directory+[IO.Path]::DirectorySeparatorChar,[StringComparison]::Ordinal)) { throw 'Dependency declared license escapes its package.' }
                if ($declared -notin @($files.FullName)) { $files+=Get-Item -LiteralPath $declared }
            }
            $fallback=$null
            if ($files.Count -eq 0) {
                $fallback=Get-DependencyFallback $dependency $root
                $copies=$fallback.files
            } else {
                $copies=@($files|ForEach-Object {@{source_path=$_.FullName;filename=$_.Name}})
            }
            $paths=@()
            foreach ($file in $copies) {
                $relative="licenses/rust/$($dependency.name)-$($dependency.version)/$($file.filename)"
                Add-File $file.source_path (Join-Path $resources $relative);$paths+=$relative
            }
            $notice=@{name=$dependency.name;version=$dependency.version;license=$dependency.license;files=$paths}
            if ($fallback) {
                $notice.selected_license=$fallback.selected_license;$notice.source_commit=$fallback.source_commit
                $notice.provenance=@($fallback.files|ForEach-Object {@{filename=$_.filename;url=$_.upstream_url;sha256=$_.sha256}})
            }
            $notice.covered_source=@(Add-DependencyCoveredSource $dependency $root $resources)
            $notices.Add($notice)
        }
        [IO.File]::WriteAllText((Join-Path $resources 'licenses/dependencies.json'),($notices|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
        $tracked=@(& git -C $root ls-files)
        if ($LASTEXITCODE -ne 0) { throw 'Tracked source inventory is unavailable.' }
        $source=@($tracked|Where-Object {$_ -match '^(src/|desktop/|resources/|tests/|scripts/|docs/|skills/|plugins/|examples/|assets/brand/|reports/|\.github/|Cargo\.toml$|Cargo\.lock$|rust-toolchain\.toml$|LICENSE$|THIRD_PARTY_NOTICES\.md$|README[^/]*\.md$|CONTEXT\.md$|CONTEXT-MAP\.md$|\.gitignore$|\.gitattributes$|run\.ps1$|plugin\.json$|\.(claude|codex|cursor)-plugin/plugin\.json$)' -and $_ -notmatch $protected -and $_ -notmatch '^desktop/(generated|binaries|gen|icons|permissions/autogenerated)/'})
        if ($source.Count -lt 20) { throw 'Matching source archive allowlist is incomplete.' }
        foreach($required in @('desktop/src/main.rs','Cargo.lock','run.ps1','plugin.json','.claude-plugin/plugin.json','.codex-plugin/plugin.json','.cursor-plugin/plugin.json','assets/brand/logo.png')){
            if($source -cnotcontains $required){throw 'Matching source archive omits a required public payload.'}
        }
        $archive=Join-Path $resources "source/numera-cha-0.3.0-source-$sha.zip"
        [IO.Directory]::CreateDirectory((Split-Path -Parent $archive))|Out-Null
        Assert-Exit (Invoke-Hosted git (@('-C',$root,'archive','--format=zip',"--output=$archive",'HEAD','--')+$source) 60 30) 'matching-source-archive'
        $zip=[IO.Compression.ZipFile]::OpenRead($archive)
        try { foreach ($entry in $zip.Entries) { if ($entry.FullName -match $protected) { throw 'Matching source archive includes protected content.' } } } finally { $zip.Dispose() }
        [IO.File]::WriteAllText((Join-Path $resources 'source/REVISION.txt'),"$sha`n",[Text.UTF8Encoding]::new($false))
        Write-Manifest $resources 'desktop-resources'
    } else {
        $stage=Join-Path $root ('.ci-work/packages/'+[guid]::NewGuid().ToString('N'))
        [IO.Directory]::CreateDirectory($stage)|Out-Null
        foreach ($name in @('numera-cha','numera-cha-solver-mcp')) { Add-File (Join-Path $release "$name$suffix") (Join-Path $stage "bin/$name$suffix") }
        foreach ($name in @('LICENSE','THIRD_PARTY_NOTICES.md','README.md','CONTEXT.md','plugin.json','.claude-plugin/plugin.json','.codex-plugin/plugin.json','.cursor-plugin/plugin.json')) { Add-File (Join-Path $root $name) (Join-Path $stage $name) }
        if ($IsWindows) { Add-File (Join-Path $root 'run.ps1') (Join-Path $stage 'run.ps1') }
        foreach ($name in @('models.json','capabilities.json')) { Add-File (Join-Path $root "resources/$name") (Join-Path $stage "resources/$name") }
        Add-File (Join-Path $root 'tests/fixtures/ocr/ascii-smoke.png') (Join-Path $stage 'tests/fixtures/ocr/ascii-smoke.png')
        foreach ($directory in @('resources/web','skills','plugins','examples','docs','reports','assets/brand')) {
            $sourceDirectory=Join-Path $root $directory
            foreach ($file in (Get-ChildItem -LiteralPath $sourceDirectory -File -Recurse -Force)) {
                $relative=[IO.Path]::GetRelativePath($root,$file.FullName).Replace('\','/')
                if ($relative -match $protected -or $file.Extension -notin @('.html','.css','.js','.png','.svg','.ico','.webp','.jpg','.woff','.woff2','.md','.json','.toml','.yml','.yaml')) { throw 'Authored asset inclusion requires explicit review.' }
                Add-File $file.FullName (Join-Path $stage $relative)
            }
        }
        foreach ($directory in @('licenses','source')) { Copy-Item -LiteralPath (Join-Path $resources $directory) -Destination (Join-Path $stage $directory) -Recurse }
        [IO.Directory]::CreateDirectory((Join-Path $stage 'data'))|Out-Null
        Copy-Item -LiteralPath $runtime -Destination (Join-Path $stage 'data/runtime') -Recurse
        if ($IsWindows) {
            foreach ($name in @('Common.psm1','NativeProcessOwner.cs','setup.ps1','start.ps1','stop.ps1','update.ps1','doctor.ps1')) { Add-File (Join-Path $PSScriptRoot $name) (Join-Path $stage "scripts/$name") }
        }
        Write-Manifest $stage 'cli-agent'
        $cliArchive=Join-Path $output "numera-cha-0.3.0-$Platform.zip"
        New-CheckedZip $stage $cliArchive
        $desktopStage=Join-Path $root ('.ci-work/desktop-package/'+[guid]::NewGuid().ToString('N'))
        [IO.Directory]::CreateDirectory($desktopStage)|Out-Null
        if ($Platform -eq 'macos-arm64') {
            $bundle=Join-Path $release 'bundle/macos/Numera CHA-Solver.app'
            if (-not [IO.Directory]::Exists($bundle)) { throw 'Native macOS application bundle is unavailable.' }
            foreach ($file in (Get-ChildItem -LiteralPath $bundle -File -Recurse -Force)) {
                $relative=[IO.Path]::GetRelativePath($bundle,$file.FullName).Replace('\','/')
                if ($relative -match '(^|/)(\.ai|\.specify|specs|\.claude|\.codex|\.cursor|\.kiro|secrets\.md|\.env[^/]*|api-token\.txt|mcp-token\.txt)(/|$)' -or $file.Extension -eq '.onnx') { throw 'macOS application includes a protected entry.' }
            }
            $null=Assert-RegularFile (Join-Path $bundle 'Contents/MacOS/numera-cha')
            $null=Assert-RegularFile (Join-Path $bundle 'Contents/Resources/data/runtime/runtime-manifest.json')
            Assert-Exit (Invoke-Hosted codesign @('--verify','--deep','--strict',$bundle) 30 15) 'macos-ad-hoc-verification'
            Assert-Exit (Invoke-Hosted ditto @('-c','-k','--sequesterRsrc','--keepParent',$bundle,(Join-Path $desktopStage 'Numera CHA-Solver.app.zip')) 120 45) 'macos-bundle-archive'
        } else {
            $folder=if ($IsWindows) {'nsis'} else {'deb'}
            $extension=if ($IsWindows) {'.exe'} else {'.deb'}
            $installers=@(Get-ChildItem -LiteralPath (Join-Path $release "bundle/$folder") -File|Where-Object {$_.Extension -eq $extension})
            if ($installers.Count -ne 1) { throw 'Expected exactly one native desktop installer.' }
            $inspect=Join-Path $root ('.ci-work/installer-inspection/'+[guid]::NewGuid().ToString('N'))
            [IO.Directory]::CreateDirectory($inspect)|Out-Null
            if ($IsWindows) {
                Assert-Exit (Invoke-Hosted 7z @('x','-y',"-o$inspect",$installers[0].FullName) 60 30) 'windows-installer-inclusion-inspection'
            } else {
                Assert-Exit (Invoke-Hosted dpkg-deb @('-x',$installers[0].FullName,$inspect) 45 20) 'linux-installer-inclusion-inspection'
            }
            $contents=@(Get-ChildItem -LiteralPath $inspect -File -Recurse -Force)
            if (-not @($contents|Where-Object {$_.Name -eq "numera-cha$suffix"}).Count -or -not @($contents|Where-Object {$_.Name -eq 'runtime-manifest.json'}).Count -or -not @($contents|Where-Object {$_.Name -eq 'ThirdPartyNotices.txt'}).Count) { throw 'Desktop installer is missing its worker/runtime/notices.' }
            foreach ($file in $contents) {
                $relative=[IO.Path]::GetRelativePath($inspect,$file.FullName).Replace('\','/')
                if ($relative -match '(^|/)(\.ai|\.specify|specs|\.claude|\.codex|\.cursor|\.kiro|\.ignoreme|secrets\.md|explain-AI\.md|\.env[^/]*|api-token\.txt|mcp-token\.txt|server\.json)(/|$)' -or $file.Extension -eq '.onnx') { throw 'Desktop installer includes a protected file or model weights.' }
            }
            Add-File $installers[0].FullName (Join-Path $desktopStage $installers[0].Name)
        }
        foreach ($directory in @('licenses','source')) { Copy-Item -LiteralPath (Join-Path $resources $directory) -Destination (Join-Path $desktopStage $directory) -Recurse }
        foreach ($name in @('LICENSE','THIRD_PARTY_NOTICES.md')) { Add-File (Join-Path $root $name) (Join-Path $desktopStage $name) }
        Add-File (Join-Path $root 'docs/DESKTOP.md') (Join-Path $desktopStage 'DESKTOP.md')
        Write-Manifest $desktopStage 'desktop-installer'
        New-CheckedZip $desktopStage (Join-Path $output "numera-cha-desktop-0.3.0-$Platform.zip")
        $hashes=@(Get-ChildItem -LiteralPath $output -File|ForEach-Object { "$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant())  $($_.Name)" })
        [IO.File]::WriteAllLines((Join-Path $output 'SHA256SUMS.txt'),$hashes,[Text.UTF8Encoding]::new($false))
        Write-HostedLog INFO verification 'Native archives verified; matching source, hashes and independent licenses included; model weights and user data excluded.'
    }
} catch { $code=1;Write-HostedLog ERROR packaging $_.Exception.Message }
finally { Write-HostedLog INFO cleanup "Hosted packaging finished exit_code=$code." }
exit $code
