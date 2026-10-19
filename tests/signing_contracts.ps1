#requires -Version 7.0
param([Parameter(Mandatory)][string]$Artifacts)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if($env:RUNNER_ENVIRONMENT -ne 'github-hosted'){throw 'Signing regression requires hosted CI.'}
$work=Join-Path $root '.ci-work/signature-contract'
[IO.Directory]::CreateDirectory($work)|Out-Null
Import-Module (Join-Path $root 'desktop/ci/Hosted.psm1') -Force
Initialize-HostedLog 'signing-contract'
function OpenSSL([string[]]$Arguments,[bool]$Pass=$true){
 $info=[Diagnostics.ProcessStartInfo]::new();$info.FileName=(Get-HostedCommand openssl)
 foreach($arg in $Arguments){$info.ArgumentList.Add($arg)}
 $info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardInput=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
 $process=[Diagnostics.Process]::new();$process.StartInfo=$info
 try{$null=$process.Start();$process.StandardInput.Close();$out=$process.StandardOutput.ReadToEndAsync();$err=$process.StandardError.ReadToEndAsync();if(-not $process.WaitForExit(15000)){$process.Kill($true);$null=$process.WaitForExit(5000);throw 'Signature regression operation timed out.'};if(($process.ExitCode -eq 0) -ne $Pass){throw 'Cryptographic signature regression returned an incorrect result.'}}finally{$process.Dispose()}
}
$archive=Get-ChildItem -LiteralPath $Artifacts -File -Filter '*.zip'|Select-Object -First 1
if(-not $archive){throw 'A signed native archive is required.'}
$public=Join-Path $root 'resources/signing/publisher-public.pem'
OpenSSL @('dgst','-sha256','-verify',$public,'-signature',($archive.FullName+'.sig'),$archive.FullName)
$altered=Join-Path $work 'altered.zip'
[IO.File]::Copy($archive.FullName,$altered,$false)
$stream=[IO.File]::Open($altered,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite)
try{$byte=$stream.ReadByte();$stream.Position=0;$stream.WriteByte($byte -bxor 1)}finally{$stream.Dispose()}
OpenSSL @('dgst','-sha256','-verify',$public,'-signature',($archive.FullName+'.sig'),$altered) $false
$rsa=[Security.Cryptography.RSA]::Create(2048)
try{[IO.File]::WriteAllText((Join-Path $work 'wrong-public.pem'),$rsa.ExportSubjectPublicKeyInfoPem(),[Text.UTF8Encoding]::new($false))}finally{$rsa.Dispose()}
OpenSSL @('dgst','-sha256','-verify',(Join-Path $work 'wrong-public.pem'),'-signature',($archive.FullName+'.sig'),$archive.FullName) $false
if($IsWindows){
 $binary=Join-Path $root 'target/release/numera-cha.exe'
 & (Join-Path $root 'scripts/verify_windows_signature.ps1') -Path $binary
 $wrongPinRejected=$false
 try{[NumeraPinnedAuthenticode]::Verify($binary,('0'*64),('0'*40))}catch{$wrongPinRejected=$true}
 if(-not $wrongPinRejected){throw 'An unapproved native signer pin was accepted.'}
 $tampered=Join-Path $work 'altered.exe';[IO.File]::Copy($binary,$tampered,$false)
 $stream=[IO.File]::Open($tampered,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite)
 try{$stream.Position=128;$byte=$stream.ReadByte();$stream.Position=128;$stream.WriteByte($byte -bxor 1)}finally{$stream.Dispose()}
 $rejected=$false
 try{& (Join-Path $root 'scripts/verify_windows_signature.ps1') -Path $tampered}catch{$rejected=$true}
 if(-not $rejected){throw 'Tampered PE was accepted.'}
}
Write-HostedLog INFO verification 'Pinned signer passed; modified archives, wrong key and applicable modified native executable rejected.'
