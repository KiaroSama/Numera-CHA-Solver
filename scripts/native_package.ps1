#requires -Version 7.0
[CmdletBinding()]
param([string]$BinaryPath='',[string]$OutputDirectory='',[switch]$Build,[switch]$SkipRuntime)
Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -Force
Initialize-RunLog 'native-package'
$code=0;$stage=$null;$archivePath=$null;$archiveCreated=$false
try{
    $root=Get-ProjectRoot
    Initialize-NativeEnvironment
    $cargo=Get-RustTool cargo
    if($Build){
        $code=Invoke-BoundedCommand -Executable $cargo -Arguments @('build','--locked','--release','--bin','numera-cha','--jobs','2') -WallSeconds 1200 -IdleSeconds 180
        if($code -ne 0){throw 'Native release build failed.'}
    }
    if(-not $BinaryPath){$BinaryPath=Join-Path $root 'target/release/numera-cha.exe'}
    if(-not [IO.Path]::IsPathRooted($BinaryPath)){$BinaryPath=Join-Path $root $BinaryPath}
    $BinaryPath=[IO.Path]::GetFullPath($BinaryPath)
    if(-not [IO.File]::Exists($BinaryPath)){throw 'Native package executable is unavailable.'}
    if(-not $OutputDirectory){$OutputDirectory=Join-Path $root '.ci-work/native-packages'}
    if(-not [IO.Path]::IsPathRooted($OutputDirectory)){$OutputDirectory=Join-Path $root $OutputDirectory}
    $OutputDirectory=[IO.Path]::GetFullPath($OutputDirectory)
    $prefix=$root.TrimEnd('\','/')+[IO.Path]::DirectorySeparatorChar
    if(-not $OutputDirectory.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)){throw 'Native package output must remain inside this project.'}
    for($directory=$OutputDirectory;$directory;$directory=Split-Path -Parent $directory){
        if((Test-Path -LiteralPath $directory) -and ((Get-Item -LiteralPath $directory -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Native package output cannot traverse a linked directory.'}
    }
    [IO.Directory]::CreateDirectory($OutputDirectory)|Out-Null
    $stage=Join-Path $OutputDirectory ('stage-'+[guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($stage)|Out-Null
    $entries=[Collections.Generic.List[object]]::new()
    function Add-PackageFile([string]$Source,[string]$Relative){
        if($Relative -match '(^|/)(\.{1,2}|secrets\.md|\.env[^/]*|explain-AI\.md|\.ignoreme)(/|$)' -or $Relative.Contains('\')){throw 'Package entry violates the native allowlist.'}
        $item=Get-Item -LiteralPath $Source -Force
        if($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Package input must be a regular non-linked file.'}
        $destination=Join-Path $stage $Relative
        [IO.Directory]::CreateDirectory((Split-Path -Parent $destination))|Out-Null
        [IO.File]::Copy($item.FullName,$destination,$false)
        $entries.Add(@{path=$Relative;size=$item.Length;sha256=(Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()})
    }
    Add-PackageFile $BinaryPath 'bin/numera-cha.exe'
    Add-PackageFile (Join-Path $root 'LICENSE') 'LICENSE'
    Add-PackageFile (Join-Path $root 'docs/NATIVE.md') 'docs/NATIVE.md'
    foreach($name in @('Common.psm1','NativeProcessOwner.cs','setup.ps1','start.ps1','stop.ps1','update.ps1','doctor.ps1')){Add-PackageFile (Join-Path $PSScriptRoot $name) "scripts/$name"}
    foreach($name in @('models.json','capabilities.json')){Add-PackageFile (Join-Path $root "resources/$name") "resources/$name"}
    $web=Join-Path $root 'resources/web'
    foreach($file in (Get-ChildItem -LiteralPath $web -File -Recurse -Force)){
        $relative=[IO.Path]::GetRelativePath($web,$file.FullName).Replace('\','/')
        if($file.Extension -notin @('.html','.css','.js','.png','.svg','.ico','.webp','.jpg','.jpeg','.woff','.woff2')){throw 'Unexpected web resource extension; review before packaging.'}
        Add-PackageFile $file.FullName "resources/web/$relative"
    }
    if(-not $SkipRuntime){
        foreach($name in @('onnxruntime.dll','onnxruntime_providers_shared.dll','onnxruntime_providers_cuda.dll')){Add-PackageFile (Join-Path $root "data/runtime/$name") "data/runtime/$name"}
        foreach($name in @('LICENSE','ThirdPartyNotices.txt')){Add-PackageFile (Join-Path $root "data/runtime/licenses/$name") "data/runtime/licenses/$name"}
    }
    $metadataPath=Join-Path $stage 'cargo-metadata.json'
    $code=Invoke-BoundedCommand -Executable $cargo -Arguments @('metadata','--locked','--offline','--format-version','1','--filter-platform','x86_64-pc-windows-msvc') -WallSeconds 60 -IdleSeconds 30 -CapturePath $metadataPath
    if($code -ne 0){throw 'Locked dependency metadata could not be collected.'}
    $metadata=[IO.File]::ReadAllText($metadataPath,[Text.Encoding]::UTF8)|ConvertFrom-Json
    [IO.File]::Delete($metadataPath)
    $package=$metadata.packages|Where-Object{$_.name -eq 'numera-cha-solver'}|Select-Object -First 1
    if(-not $package){throw 'Application metadata is missing.'}
    $notices=[Collections.Generic.List[object]]::new()
    $resolved=@($metadata.resolve.nodes|ForEach-Object{$_.id})
    foreach($dependency in $metadata.packages){
        if($dependency.id -eq $package.id -or $dependency.id -notin $resolved){continue}
        $directory=Split-Path -Parent $dependency.manifest_path
        $files=@(Get-ChildItem -LiteralPath $directory -File -Force|Where-Object{$_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE|UNLICENSE)([._-].*)?$'})
        if($dependency.license_file){
            $declared=Join-Path $directory $dependency.license_file
            if(-not (@($files|ForEach-Object{$_.FullName}) -contains $declared)){$files+=Get-Item -LiteralPath $declared}
        }
        if($files.Count -eq 0){throw "Dependency license text is missing for $($dependency.name); publication is blocked."}
        $licensePaths=@()
        foreach($file in $files){
            $relative="licenses/rust/$($dependency.name)-$($dependency.version)/$($file.Name)"
            Add-PackageFile $file.FullName $relative;$licensePaths+=$relative
        }
        $notices.Add(@{name=$dependency.name;version=$dependency.version;license=$dependency.license;files=$licensePaths})
    }
    $noticePath=Join-Path $stage 'licenses/dependencies.json'
    [IO.File]::WriteAllText($noticePath,($notices|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
    $entries.Add(@{path='licenses/dependencies.json';size=(Get-Item $noticePath).Length;sha256=(Get-FileHash $noticePath -Algorithm SHA256).Hash.ToLowerInvariant()})
    $manifest=@{schema_version=1;version=$package.version;platform='windows-x64';runtime_included=(-not $SkipRuntime);files=$entries.ToArray()}
    $manifestPath=Join-Path $stage 'package-manifest.json'
    [IO.File]::WriteAllText($manifestPath,($manifest|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
    $archivePath=Join-Path $OutputDirectory "numera-cha-$($package.version)-windows-x64.zip"
    if([IO.File]::Exists($archivePath)){throw 'Native package already exists; choose a clean owned output directory.'}
    $stream=[IO.File]::Open($archivePath,[IO.FileMode]::CreateNew);$archiveCreated=$true
    $archive=[IO.Compression.ZipArchive]::new($stream,[IO.Compression.ZipArchiveMode]::Create,$false)
    try{
        foreach($entry in $entries){[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive,(Join-Path $stage $entry.path),$entry.path,[IO.Compression.CompressionLevel]::Optimal)|Out-Null}
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive,$manifestPath,'package-manifest.json',[IO.Compression.CompressionLevel]::Optimal)|Out-Null
    }finally{$archive.Dispose();$stream.Dispose()}
    $archive=[IO.Compression.ZipFile]::OpenRead($archivePath)
    try{
        $allowed=@($entries|ForEach-Object{$_.path})+@('package-manifest.json')
        if($archive.Entries.Count -ne $allowed.Count){throw 'Native package entry count differs from the allowlist.'}
        foreach($entry in $archive.Entries){
            if($entry.FullName -notin $allowed){throw 'Native package contains an unreviewed entry.'}
            if($entry.FullName -eq 'package-manifest.json'){continue}
            $expected=$entries|Where-Object{$_.path -eq $entry.FullName}|Select-Object -First 1
            $input=$entry.Open();$hash=[Security.Cryptography.SHA256]::Create()
            try{$actual=[Convert]::ToHexString($hash.ComputeHash($input)).ToLowerInvariant()}finally{$hash.Dispose();$input.Dispose()}
            if($actual -ne $expected.sha256 -or $entry.Length -ne $expected.size){throw 'Native package asset hash or length differs.'}
        }
    }finally{$archive.Dispose()}
    Write-RunLog INFO packaging 'Native package verified against its exact allowlist and SHA-256 manifest; models and user data excluded.'
    [Console]::Out.WriteLine($archivePath)
}catch{if($code -eq 0){$code=1};Write-RunLog ERROR packaging $_.Exception.Message;if($archiveCreated -and $archivePath -and [IO.File]::Exists($archivePath)){[IO.File]::Delete($archivePath)}}
finally{if($stage -and [IO.Directory]::Exists($stage)){[IO.Directory]::Delete($stage,$true)};Write-RunLog INFO cleanup "Native packaging finished exit_code=$code."}
exit $code
