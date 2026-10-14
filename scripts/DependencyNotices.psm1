#requires -Version 7.0
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:Catalog = $null

function Initialize-NoticeCatalog {
    if (-not $script:Catalog) {
        $script:Catalog=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'license_fallbacks.json'),[Text.Encoding]::UTF8)|ConvertFrom-Json
        if ($script:Catalog.schema_version -ne 1) { throw 'Unsupported dependency notice catalog.' }
    }
}
function Get-DependencyFallback([object]$Dependency,[string]$Root) {
    Initialize-NoticeCatalog
    $matches=@($script:Catalog.crates|Where-Object {$_.name -ceq $Dependency.name -and $_.version -ceq $Dependency.version})
    if ($matches.Count -ne 1) { throw "Dependency license text is unavailable: $($Dependency.name) $($Dependency.version)." }
    $entry=$matches[0]
    if ($entry.license_expression -cne $Dependency.license) { throw 'Dependency license expression differs from its reviewed pin.' }
    $directory=Split-Path -Parent $Dependency.manifest_path
    $vcs=[IO.File]::ReadAllText((Join-Path $directory '.cargo_vcs_info.json'),[Text.Encoding]::UTF8)|ConvertFrom-Json
    if ($vcs.git.sha1 -cne $entry.source_commit) { throw 'Dependency source revision differs from its reviewed notice.' }
    $files=@(foreach ($file in $entry.files) {
        if ($file.filename -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]*$') { throw 'Invalid dependency notice filename.' }
        $source=$script:Catalog.sources.PSObject.Properties[$file.source].Value
        if (-not $source -or $source.path -notmatch '^resources/licenses/upstream/[A-Za-z0-9][A-Za-z0-9._-]*$' -or $source.sha256 -notmatch '^[a-f0-9]{64}$') { throw 'Invalid reviewed dependency notice source.' }
        $path=Join-Path $Root $source.path
        $null=Assert-RegularFile $path
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -cne $source.sha256) { throw 'Reviewed dependency notice bytes differ from their pin.' }
        @{source_path=$path;filename=$file.filename;upstream_url=$source.url;sha256=$source.sha256}
    })
    if (-not $files.Count) { throw 'Reviewed dependency notice has no complete texts.' }
    return @{files=$files;selected_license=$entry.selected_license;source_commit=$entry.source_commit;entry=$entry}
}

function Add-DependencyCoveredSource([object]$Dependency,[string]$Root,[string]$Resources) {
    if ($Dependency.license -cne 'MPL-2.0') { return @() }
    Assert-Hosted
    Initialize-NoticeCatalog
    $matches=@($script:Catalog.covered_source|Where-Object {$_.name -ceq $Dependency.name -and $_.version -ceq $Dependency.version})
    if ($matches.Count -ne 1) { throw 'Covered dependency source has no reviewed exact-version pin.' }
    $entry=$matches[0]
    $source=$script:Catalog.sources.PSObject.Properties[$entry.source].Value
    $expected="https://static.crates.io/crates/$($Dependency.name)/$($Dependency.name)-$($Dependency.version).crate"
    if ($source.url -cne $expected -or $source.sha256 -notmatch '^[a-f0-9]{64}$' -or $entry.filename -cne "$($Dependency.name)-$($Dependency.version).crate") { throw 'Invalid covered-source archive pin.' }
    $lock=[IO.File]::ReadAllText((Join-Path $Root 'Cargo.lock'),[Text.Encoding]::UTF8)
    $sections=@([regex]::Matches($lock,'(?ms)^\[\[package\]\]\r?\n(?:(?!^\[\[package\]\]).)*')|ForEach-Object {$_.Value}|Where-Object {
        $_ -match ('(?m)^name = "'+[regex]::Escape($Dependency.name)+'"\r?$') -and $_ -match ('(?m)^version = "'+[regex]::Escape($Dependency.version)+'"\r?$')
    })
    if ($sections.Count -ne 1 -or $sections[0] -notmatch ('(?m)^checksum = "'+$source.sha256+'"\r?$')) { throw 'Covered-source archive differs from the locked registry package.' }
    $directory=Split-Path -Parent $Dependency.manifest_path
    $vcs=[IO.File]::ReadAllText((Join-Path $directory '.cargo_vcs_info.json'),[Text.Encoding]::UTF8)|ConvertFrom-Json
    if ($vcs.git.sha1 -cne $entry.source_commit) { throw 'Covered-source revision differs from its reviewed pin.' }
    $work=Join-Path $Root ('.ci-work/covered-source/'+[guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($work)|Out-Null
    $download=Join-Path $work $entry.filename
    # Keep the original archive opaque: no extraction, import or execution of downloaded source.
    Invoke-WebRequest -Uri $source.url -OutFile $download -ConnectionTimeoutSeconds 15 -OperationTimeoutSeconds 45
    $item=Assert-RegularFile $download
    if ($item.Length -gt 8MB -or (Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash.ToLowerInvariant() -cne $source.sha256) { throw 'Covered-source archive bytes differ from their reviewed pin.' }
    $relative="source/third-party/$($Dependency.name)-$($Dependency.version)/$($entry.filename)"
    $destination=Join-Path $Resources $relative
    [IO.Directory]::CreateDirectory((Split-Path -Parent $destination))|Out-Null
    [IO.File]::Copy($download,$destination,$false)
    $licenses=@(Get-ChildItem -LiteralPath (Join-Path $Resources "licenses/rust/$($Dependency.name)-$($Dependency.version)") -File)
    if (-not $licenses.Count) { throw 'Covered-source distribution has no accompanying license text.' }
    $licensePath=[IO.Path]::GetRelativePath((Split-Path -Parent $destination),$licenses[0].FullName).Replace('\','/')
    $notice=@"
$($Dependency.name) $($Dependency.version) covered source is included unchanged in $($entry.filename).
Source: $($source.url)
SHA256: $($source.sha256)
Upstream revision: $($entry.source_commit)
Extract this gzip-compressed tar archive with a standard archive tool to obtain the preferred source form.
The included source and its original notices remain governed by MPL-2.0, independently of Numera's license.
Numera's noncommercial restrictions do not limit your MPL rights to this covered source.
The full license is included at $licensePath.
"@
    [IO.File]::WriteAllText((Join-Path (Split-Path -Parent $destination) 'SOURCE-NOTICE.txt'),$notice+"`n",[Text.UTF8Encoding]::new($false))
    return @($relative,[IO.Path]::GetRelativePath($Resources,(Join-Path (Split-Path -Parent $destination) 'SOURCE-NOTICE.txt')).Replace('\','/'))
}
Export-ModuleMember -Function Get-DependencyFallback,Add-DependencyCoveredSource
