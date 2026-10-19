#requires -Version 7.0
param([Parameter(Mandatory)][ValidateSet('windows-x64','linux-x64','macos-arm64')][string]$Platform)
Import-Module (Join-Path $PSScriptRoot '../desktop/ci/Hosted.psm1') -Force
Initialize-HostedLog 'prepare-browser'
$root=Get-HostedRoot
$version='155.0.8059.84'
$target=@{'windows-x64'='win64';'linux-x64'='linux64';'macos-arm64'='mac-arm64'}[$Platform]
$directory=Join-Path $root '.ci-work/browser-tools'
[IO.Directory]::CreateDirectory($directory)|Out-Null
$browserKind=if($Platform -eq 'windows-x64'){'chrome'}else{'chrome-headless-shell'}
foreach($kind in @($browserKind,'chromedriver')){
 $archive=Join-Path $directory "$kind.zip"
 $url="https://storage.googleapis.com/chrome-for-testing-public/$version/$target/$kind-$target.zip"
 Invoke-WebRequest -Uri $url -OutFile $archive -ConnectionTimeoutSeconds 20 -OperationTimeoutSeconds 180
 $zip=[IO.Compression.ZipFile]::OpenRead($archive)
 try{
  foreach($entry in $zip.Entries){
   if($entry.FullName -match '(^/|^[A-Za-z]:|(^|[\\/])\.\.([\\/]|$))' -or (($entry.ExternalAttributes -shr 16) -band 0xf000) -eq 0xa000){throw 'Browser archive has an unsafe path or link.'}
  }
 }finally{$zip.Dispose()}
 [IO.Compression.ZipFile]::ExtractToDirectory($archive,$directory)
}
$browser=Join-Path $directory ("$browserKind-$target/"+$(if($IsWindows){'chrome.exe'}else{'chrome-headless-shell'}))
$driver=Join-Path $directory ("chromedriver-$target/"+$(if($IsWindows){'chromedriver.exe'}else{'chromedriver'}))
if(-not $IsWindows){
 foreach($file in Get-ChildItem -LiteralPath $directory -File -Recurse){
  # CfT executables/helpers require their executable mode after ZIP extraction.
  if($file.Extension -eq '' -or $file.Name -in @('chrome_crashpad_handler','chrome-sandbox') -or $file.FullName -like '*/Contents/MacOS/*'){
   [IO.File]::SetUnixFileMode($file.FullName,[IO.UnixFileMode]::UserRead -bor [IO.UnixFileMode]::UserWrite -bor [IO.UnixFileMode]::UserExecute -bor [IO.UnixFileMode]::GroupRead -bor [IO.UnixFileMode]::GroupExecute -bor [IO.UnixFileMode]::OtherRead -bor [IO.UnixFileMode]::OtherExecute)
  }
 }
}
if($Platform -eq 'linux-x64'){
 # The official installed binary already has Ubuntu's path-scoped AppArmor allowance.
 $browser='/opt/google/chrome/chrome'
 $null=Assert-RegularFile $browser
 $installedVersion=(& $browser --product-version).Trim()
 if($LASTEXITCODE -ne 0 -or $installedVersion -notmatch '^155\.0\.8059\.[0-9]+$'){throw 'Installed sandboxed Chrome differs from the reviewed driver build family.'}
 Write-HostedLog INFO verification "Using official installed Chrome version=$installedVersion with its existing sandbox policy."
}
foreach($file in @($browser,$driver)){$null=Assert-RegularFile $file}
"NUMERA_BROWSER_EXECUTABLE=$browser"|Out-File $env:GITHUB_ENV -Append -Encoding utf8
"NUMERA_BROWSER_DRIVER=$driver"|Out-File $env:GITHUB_ENV -Append -Encoding utf8
Write-HostedLog INFO verification 'Official matching stable Chrome/ChromeDriver staged; no Python or ordinary browser profile used.'
