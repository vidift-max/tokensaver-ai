[CmdletBinding()]
param(
 [Parameter(Mandatory=$true)][string]$MsixPath,
 [Parameter(Mandatory=$true)][string]$ExpectedPublisher,
 [Parameter(Mandatory=$true)][string]$ExpectedPublisherDisplayName,
 [Parameter(Mandatory=$true)][string]$ApprovedThumbprint,
 [Parameter(Mandatory=$true)][string]$ManifestPath,
 [Parameter(Mandatory=$true)][string]$ProvenancePath,
 [Parameter(Mandatory=$true)][string]$HashManifestPath,
 [Parameter(Mandatory=$true)][string]$ExpectedUnsignedSha256,
 [string]$TimestampUrl='https://timestamp.digicert.com',
 [string]$OutputDirectory=''
)
$ErrorActionPreference='Stop'
function Block([string]$s,[string]$m){Write-Host "SIGNING_STATUS=$s";throw $m}
if($TimestampUrl -notmatch '^https://'){Block 'SIGNING_BLOCKED' 'TSA must use HTTPS'}
if($ExpectedPublisher -cne 'CN=TokenSaver Publisher'){Block 'SIGNING_BLOCKED' 'Publisher mismatch'}
if($ApprovedThumbprint -notmatch '^[A-Fa-f0-9]{40}$'){Block 'CERTIFICATE_PENDING' 'Approved thumbprint unavailable'}
foreach($p in @($MsixPath,$ManifestPath,$ProvenancePath,$HashManifestPath)){if(-not(Test-Path -LiteralPath $p -PathType Leaf)){Block 'SIGNING_BLOCKED' 'Unsigned evidence missing'}}
$hash=(Get-FileHash $MsixPath -Algorithm SHA256).Hash.ToLowerInvariant()
$prov=Get-Content $ProvenancePath -Raw|ConvertFrom-Json
if($hash -cne $ExpectedUnsignedSha256.ToLowerInvariant() -or $prov.signature_status -ne 'UNSIGNED' -or $prov.unsigned_msix_sha256 -cne $hash -or $prov.source_sha -notmatch '^[0-9a-f]{40}$'){Block 'SIGNING_BLOCKED' 'Unsigned artifact/provenance mismatch'}
[xml]$manifest=Get-Content $ManifestPath -Raw;$id=$manifest.Package.Identity
if($id.Publisher -cne $ExpectedPublisher -or $manifest.Package.Properties.PublisherDisplayName -cne $ExpectedPublisherDisplayName -or $id.Name -cne 'TokenSaver.Phone02.Supervisor' -or $id.ProcessorArchitecture -cne 'x64'){Block 'SIGNING_BLOCKED' 'MSIX identity mismatch'}
$cert=Get-ChildItem Cert:\LocalMachine\My|Where-Object{$_.Thumbprint -ceq $ApprovedThumbprint.ToUpperInvariant()}|Select-Object -First 1
if($null -eq $cert){Block 'CERTIFICATE_PENDING' 'Certificate absent from LocalMachine\My'}
if($cert.Subject -cne $ExpectedPublisher -or $cert.Thumbprint -cne $ApprovedThumbprint.ToUpperInvariant()){Block 'SIGNING_BLOCKED' 'Certificate Subject/thumbprint mismatch'}
$now=[DateTime]::UtcNow
if($now -lt $cert.NotBefore.ToUniversalTime() -or $now -gt $cert.NotAfter.ToUniversalTime()){Block 'SIGNING_BLOCKED' 'Certificate outside validity'}
if(-not $cert.HasPrivateKey){Block 'SIGNING_BLOCKED' 'Private key unavailable'}
$eku=@($cert.Extensions|Where-Object{$_ -is [System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]}|ForEach-Object{$_.EnhancedKeyUsages}|ForEach-Object{$_.Value})
if($eku -notcontains '1.3.6.1.5.5.7.3.3'){Block 'SIGNING_BLOCKED' 'Code Signing EKU missing'}
$chain=[System.Security.Cryptography.X509Certificates.X509Chain]::new();$chain.ChainPolicy.RevocationMode='Online';$chain.ChainPolicy.RevocationFlag='ExcludeRoot'
if(-not $chain.Build($cert)){Block 'SIGNING_BLOCKED' 'Certificate chain/revocation validation failed'}
$pf=[Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFilesX86)
$sdk=Get-ChildItem (Join-Path $pf 'Windows Kits\10\bin') -Directory|Where-Object Name -Match '^10\\.'|Sort-Object Name -Descending|Select-Object -First 1
$st=if($sdk){Join-Path $sdk.FullName 'x64\signtool.exe'}else{$null}
if(-not $st -or -not(Test-Path $st)){Block 'SIGNING_BLOCKED' 'SignTool unavailable'}
if(-not $OutputDirectory){$OutputDirectory=Split-Path -Parent $MsixPath};New-Item -ItemType Directory -Force $OutputDirectory|Out-Null
$signed=Join-Path $OutputDirectory 'TokenSaver.Phone02.Supervisor.signed.msix';Copy-Item $MsixPath $signed -Force
# Uses Windows certificate store private key. No PFX, password, /f or /p.
& $st sign /fd SHA256 /sha1 $cert.Thumbprint /s My /sm /tr $TimestampUrl /td SHA256 $signed
if($LASTEXITCODE -ne 0){Block 'SIGNING_BLOCKED' 'SignTool failed'}
$meta=[ordered]@{source_sha=$prov.source_sha;unsigned_msix_sha256=$hash;signed_msix_sha256=(Get-FileHash $signed -Algorithm SHA256).Hash.ToLowerInvariant();package_identity=$id.Name;publisher=$ExpectedPublisher;publisher_display_name=$ExpectedPublisherDisplayName;certificate_thumbprint=$cert.Thumbprint;certificate_subject=$cert.Subject;certificate_not_before_utc=$cert.NotBefore.ToUniversalTime().ToString('o');certificate_not_after_utc=$cert.NotAfter.ToUniversalTime().ToString('o');certificate_chain_status='BUILT_AND_TRUSTED';tsa_url=$TimestampUrl;timestamp_status='PENDING_CRYPTOGRAPHIC_VERIFY';signature_status='PENDING_VERIFICATION';verification_status='PENDING'}
$meta|ConvertTo-Json -Depth 8|Set-Content (Join-Path $OutputDirectory 'provenance-signed.json') -Encoding utf8
Write-Host "UNSIGNED_MSIX_SHA256=$hash";Write-Host "SIGNED_MSIX_SHA256=$($meta.signed_msix_sha256)";Write-Host "PUBLISHER=$ExpectedPublisher";Write-Host "CERTIFICATE_THUMBPRINT=$($cert.Thumbprint)";Write-Host 'SIGNATURE_STATUS=SIGNATURE_PENDING'
