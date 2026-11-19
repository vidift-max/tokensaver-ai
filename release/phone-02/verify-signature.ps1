[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$MsixPath,[Parameter(Mandatory=$true)][string]$MetadataPath,[Parameter(Mandatory=$true)][string]$ExpectedPublisher,[Parameter(Mandatory=$true)][string]$ExpectedPublisherDisplayName,[string]$TimestampUrl='https://timestamp.digicert.com')
$ErrorActionPreference='Stop'
function Invalid([string]$m){Write-Host 'SIGNATURE_STATUS=SIGNATURE_INVALID';throw $m}
if(-not(Test-Path $MsixPath -PathType Leaf) -or -not(Test-Path $MetadataPath -PathType Leaf)){Invalid 'Signed artifact/metadata missing'}
if($TimestampUrl -notmatch '^https://'){Invalid 'TSA must use HTTPS'}
$m=Get-Content $MetadataPath -Raw|ConvertFrom-Json
if($m.publisher -cne $ExpectedPublisher -or $m.publisher_display_name -cne $ExpectedPublisherDisplayName -or $m.package_identity -cne 'TokenSaver.Phone02.Supervisor'){Invalid 'Publisher/identity mismatch'}
$hash=(Get-FileHash $MsixPath -Algorithm SHA256).Hash.ToLowerInvariant();if($hash -cne $m.signed_msix_sha256){Invalid 'Signed hash mismatch'}
$sig=Get-AuthenticodeSignature $MsixPath;if($sig.Status -ne [System.Management.Automation.SignatureStatus]::Valid -or $null -eq $sig.SignerCertificate){Invalid 'Authenticode signature invalid'}
$c=$sig.SignerCertificate
if($c.Subject -cne $ExpectedPublisher -or $c.Thumbprint -cne $m.certificate_thumbprint){Invalid 'Signer Subject/thumbprint mismatch'}
$now=[DateTime]::UtcNow;if($now -lt $c.NotBefore.ToUniversalTime() -or $now -gt $c.NotAfter.ToUniversalTime()){Invalid 'Signer certificate outside validity'}
$eku=@($c.Extensions|Where-Object{$_ -is [System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]}|ForEach-Object{$_.EnhancedKeyUsages}|ForEach-Object{$_.Value})
if($eku -notcontains '1.3.6.1.5.5.7.3.3'){Invalid 'Code Signing EKU missing'}
$chain=[System.Security.Cryptography.X509Certificates.X509Chain]::new();$chain.ChainPolicy.RevocationMode='Online';$chain.ChainPolicy.RevocationFlag='ExcludeRoot'
if(-not $chain.Build($c)){Invalid 'Signer chain untrusted'}
$pf=[Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFilesX86)
$sdk=Get-ChildItem (Join-Path $pf 'Windows Kits\10\bin') -Directory|Where-Object Name -Match '^10\\.'|Sort-Object Name -Descending|Select-Object -First 1
$st=if($sdk){Join-Path $sdk.FullName 'x64\signtool.exe'}else{$null}
if(-not $st -or -not(Test-Path $st)){Write-Host 'SIGNATURE_STATUS=SIGNATURE_PENDING';throw 'TIMESTAMP_VERIFICATION_BLOCKED'}
# SignTool/WinTrust cryptographically validates Authenticode and RFC3161 token/chain.
$out=& $st verify /pa /all /tw /v $MsixPath 2>&1;$exit=$LASTEXITCODE;$txt=$out|Out-String
if($exit -ne 0){Invalid 'WinTrust signature/timestamp verification failed'}
if($txt -notmatch '(?i)(signature is timestamped|timestamp verified|timestamp is valid|timestamp verified by)'){Write-Host 'TIMESTAMP_VERIFICATION=TIMESTAMP_VERIFICATION_BLOCKED';Write-Host 'SIGNATURE_STATUS=SIGNATURE_PENDING';throw 'TIMESTAMP_VERIFICATION_BLOCKED'}
if($m.tsa_url -cne $TimestampUrl -or $m.tsa_url -notmatch '^https://'){Invalid 'TSA metadata mismatch'}
$m.signature_status='SIGNATURE_VALID';$m.verification_status='SIGNATURE_VALID';$m.timestamp_status='RFC3161_VERIFIED_BY_SIGNTOOL_WINTRUST';$m.signer_subject_verified=$c.Subject;$m.signer_thumbprint_verified=$c.Thumbprint;$m.signed_msix_sha256_verified=$hash;$m.certificate_chain_status='TRUSTED'
$m|ConvertTo-Json -Depth 8|Set-Content $MetadataPath -Encoding utf8
Write-Host "SIGNED_MSIX_SHA256=$hash";Write-Host "PUBLISHER=$($c.Subject)";Write-Host "CERTIFICATE_THUMBPRINT=$($c.Thumbprint)";Write-Host 'TIMESTAMP_VERIFICATION=RFC3161_VERIFIED_BY_SIGNTOOL_WINTRUST';Write-Host 'SIGNATURE_STATUS=SIGNATURE_VALID'
