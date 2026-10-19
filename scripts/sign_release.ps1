#requires -Version 7.0
[CmdletBinding()]
param([ValidateSet('Initialize','Native','Archives','VerifyArchives','CheckWindows')][string]$Phase='Native',[string]$Path)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$public=Join-Path $root 'resources/signing'
$pin=[IO.File]::ReadAllText((Join-Path $public 'publisher.json'),[Text.Encoding]::UTF8)|ConvertFrom-Json
$private=Join-Path $root '.ci-work/signing'
if($env:RUNNER_ENVIRONMENT -eq 'github-hosted'){
 Import-Module (Join-Path $root 'desktop/ci/Hosted.psm1') -Force
 Initialize-HostedLog 'release-signing'
}
function Invoke-SigningTool([string]$Executable,[string[]]$Arguments) {
 if($env:RUNNER_ENVIRONMENT -eq 'github-hosted'){Write-HostedLog INFO signing "Tool=$([IO.Path]::GetFileName($Executable)) operation=$($Arguments[0]) started; arguments withheld."}
 $info=[Diagnostics.ProcessStartInfo]::new();$info.FileName=$Executable
 foreach($arg in $Arguments){$info.ArgumentList.Add($arg)}
 $info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardInput=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
 $process=[Diagnostics.Process]::new();$process.StartInfo=$info
 try{
  $null=$process.Start();$process.StandardInput.Close();$out=$process.StandardOutput.ReadToEndAsync();$err=$process.StandardError.ReadToEndAsync()
  if(-not $process.WaitForExit(30000)){$process.Kill($true);$null=$process.WaitForExit(5000);throw 'Signing operation exceeded its deadline.'}
  if($process.ExitCode -ne 0){
   $diagnostic=$err.GetAwaiter().GetResult()
   foreach($name in @('NUMERA_SIGNING_PFX','NUMERA_SIGNING_PASSWORD','NUMERA_SIGNING_KEY')){$value=[Environment]::GetEnvironmentVariable($name);if($value){$diagnostic=$diagnostic.Replace($value,'[REDACTED]')}}
   $safe=@([regex]::Matches($diagnostic,'(?m)^security: [A-Za-z][A-Za-z0-9_]+: [A-Za-z0-9 ()/.,_-]{1,160}$')|ForEach-Object {$_.Value}) -join '; '
   if([IO.Path]::GetFileName($Executable) -eq 'codesign'){
    $safe=($diagnostic -replace '(?m)^.*?:\s*','' -replace '[\r\n]+',' ').Trim()
    $safe=$safe.Substring(0,[Math]::Min(1024,$safe.Length))
   }
   throw "Signature operation failed: tool=$([IO.Path]::GetFileName($Executable)) operation=$($Arguments[0]) exit_code=$($process.ExitCode); $safe; other private output withheld."
  }
 }finally{$process.Dispose()}
}
function Get-Certificate {
 $password=[IO.File]::ReadAllText((Join-Path $private 'password.txt'),[Text.Encoding]::UTF8)
 if($IsWindows){
  $flags=[Security.Cryptography.X509Certificates.X509KeyStorageFlags]::Exportable -bor [Security.Cryptography.X509Certificates.X509KeyStorageFlags]::PersistKeySet
  $certificate=[Security.Cryptography.X509Certificates.X509Certificate2]::new((Join-Path $private 'publisher.pfx'),$password,$flags)
 }else{$certificate=[Security.Cryptography.X509Certificates.X509Certificate2]::new((Join-Path $public 'publisher-cert.pem'))}
 $hash=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($certificate.RawData)).ToLowerInvariant()
 if($hash -cne $pin.sha256 -or ($IsWindows -and -not $certificate.HasPrivateKey)){$certificate.Dispose();throw 'Private signer does not match the approved public identity.'}
 return $certificate
}
function Test-WindowsSignature([string]$File) {
 & (Join-Path $PSScriptRoot 'verify_windows_signature.ps1') -Path $File
}
if($Phase -eq 'Initialize') {
 if($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted'){throw 'Private signing initialization requires this project hosted CI.'}
 if(Test-Path -LiteralPath $private){throw 'Private signing work already exists.'}
 [IO.Directory]::CreateDirectory($private)|Out-Null
 if(-not $IsWindows){[IO.File]::SetUnixFileMode($private,[IO.UnixFileMode]::UserRead -bor [IO.UnixFileMode]::UserWrite -bor [IO.UnixFileMode]::UserExecute)}
 foreach($name in @('NUMERA_SIGNING_PFX','NUMERA_SIGNING_PASSWORD','NUMERA_SIGNING_KEY')){if(-not [Environment]::GetEnvironmentVariable($name)){throw 'Required protected signing input is unavailable.'}}
 [IO.File]::WriteAllBytes((Join-Path $private 'publisher.pfx'),[Convert]::FromBase64String($env:NUMERA_SIGNING_PFX))
 [IO.File]::WriteAllText((Join-Path $private 'password.txt'),$env:NUMERA_SIGNING_PASSWORD,[Text.UTF8Encoding]::new($false))
 [IO.File]::WriteAllText((Join-Path $private 'private.pem'),$env:NUMERA_SIGNING_KEY,[Text.UTF8Encoding]::new($false))
 foreach($file in Get-ChildItem -LiteralPath $private -File){if(-not $IsWindows){[IO.File]::SetUnixFileMode($file.FullName,[IO.UnixFileMode]::UserRead -bor [IO.UnixFileMode]::UserWrite)}}
 $certificate=Get-Certificate
 try{$key=[Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey($certificate);if($IsWindows -and $key -is [Security.Cryptography.RSACng]){$key.Key.Delete()};if($key){$key.Dispose()}}finally{$certificate.Dispose()}
 if($IsMacOS){
  $keychain=Join-Path $private 'publisher.keychain-db'
  Invoke-SigningTool security @('create-keychain','-p','',$keychain)
  Invoke-SigningTool security @('set-keychain-settings','-lut','21600',$keychain)
  Invoke-SigningTool security @('unlock-keychain','-p','',$keychain)
  # Native PEM-sequence import avoids PKCS#12 encryption compatibility and password arguments.
  $bundle=Join-Path $private 'identity.pem'
  [IO.File]::WriteAllText($bundle,([IO.File]::ReadAllText((Join-Path $private 'private.pem'),[Text.Encoding]::UTF8)+"`n"+[IO.File]::ReadAllText((Join-Path $public 'publisher-cert.pem'),[Text.Encoding]::UTF8)),[Text.UTF8Encoding]::new($false))
  [IO.File]::SetUnixFileMode($bundle,[IO.UnixFileMode]::UserRead -bor [IO.UnixFileMode]::UserWrite)
  Invoke-SigningTool security @('import',$bundle,'-f','pemseq','-t','agg','-k',$keychain,'-T','/usr/bin/codesign')
  Invoke-SigningTool security @('set-key-partition-list','-S','apple-tool:,apple:','-s','-k','',$keychain)
  $previous=@(& security list-keychains -d user|ForEach-Object {$_.Trim().Trim('"')})
  if($LASTEXITCODE -ne 0){throw 'Keychain search list could not be preserved.'}
  [IO.File]::WriteAllText((Join-Path $private 'keychains.json'),($previous|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
  Invoke-SigningTool security (@('list-keychains','-d','user','-s',$keychain)+$previous)
  # Validate the native signing contract before the expensive application build.
  $probe=Join-Path $private 'signing-probe'
  [IO.File]::Copy($env:NUMERA_CI_OWNER_BINARY,$probe,$false)
  [IO.File]::SetUnixFileMode($probe,[IO.UnixFileMode]::UserRead -bor [IO.UnixFileMode]::UserWrite -bor [IO.UnixFileMode]::UserExecute)
  Invoke-SigningTool codesign @('--force','--sign',$pin.thumbprint,'--keychain',$keychain,'--timestamp=none',$probe)
  Invoke-SigningTool codesign @('--verify','--strict','-R',('certificate leaf = H"'+$pin.thumbprint+'"'),$probe)
 }
 'Protected signing identity initialized; no public trust installed.'
} elseif($Phase -eq 'Native') {
 Write-Output "Native signing requested: $([IO.Path]::GetFileName($Path))"
 if(-not $Path -or (-not [IO.File]::Exists($Path) -and -not ($IsMacOS -and [IO.Directory]::Exists($Path) -and $Path.EndsWith('.app')))){throw 'A native file or macOS application bundle is required.'}
 if((Get-Item -LiteralPath $Path).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Linked signing inputs are refused.'}
 if($IsWindows){
  $certificate=Get-Certificate
  try{$null=Set-AuthenticodeSignature -LiteralPath $Path -Certificate $certificate -HashAlgorithm SHA256 -IncludeChain Signer}finally{
   $key=[Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey($certificate)
   try{if($key -is [Security.Cryptography.RSACng]){$key.Key.Delete()}elseif($key -is [Security.Cryptography.RSACryptoServiceProvider]){$key.PersistKeyInCsp=$false}}finally{if($key){$key.Dispose()};$certificate.Dispose()}
  }
  Test-WindowsSignature $Path
  Write-Output "Pinned native signature verified: $([IO.Path]::GetFileName($Path))"
 } elseif($IsMacOS){
  Invoke-SigningTool codesign @('--force','--sign',$pin.thumbprint,'--keychain',(Join-Path $private 'publisher.keychain-db'),'--timestamp=none',$Path)
  Invoke-SigningTool codesign @('--verify','--strict','-R',('certificate leaf = H"'+$pin.thumbprint+'"'),$Path)
 } else {throw 'Linux native files are covered by the signed archive manifest.'}
} elseif($Phase -eq 'CheckWindows') { Test-WindowsSignature $Path }
else {
 if(-not [IO.Directory]::Exists($Path)){throw 'An artifact directory is required.'}
 $files=@(Get-ChildItem -LiteralPath $Path -File -Recurse|Where-Object {$_.Extension -eq '.zip' -and $_.Name -like 'numera-cha-*'})
 if($files.Count -notin @(2,6)){throw 'Expected two platform archives or all six release archives.'}
 foreach($file in $files){
  $signature=$file.FullName+'.sig'
  if($Phase -eq 'Archives'){Invoke-SigningTool openssl @('dgst','-sha256','-sign',(Join-Path $private 'private.pem'),'-out',$signature,$file.FullName)}
  Invoke-SigningTool openssl @('dgst','-sha256','-verify',(Join-Path $public 'publisher-public.pem'),'-signature',$signature,$file.FullName)
 }
 'Archive signatures verified against the pinned project public key.'
}
