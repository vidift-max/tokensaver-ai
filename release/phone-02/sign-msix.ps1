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
 [Parameter(Mandatory=$true)][string]$ExpectedSourceSha,
 [string]$TimestampUrl='https://timestamp.digicert.com',
 [string]$OutputDirectory=''
)
$ErrorActionPreference='Stop'
function Block([string]$Status,[string]$Message){Write-Host "SIGNING_STATUS=$Status";throw $Message}
if($TimestampUrl -notmatch '^https://'){Block 'SIGNING_BLOCKED' 'TSA must use HTTPS'}
if($ExpectedPublisher -cne 'CN=TokenSaver Publisher'){Block 'SIGNING_BLOCKED' 'Publisher mismatch'}
if($ApprovedThumbprint -notmatch '^[A-Fa-f0-9]{40}$'){Block 'CERTIFICATE_PENDING' 'Approved thumbprint unavailable'}
foreach($path in @($MsixPath,$ManifestPath,$ProvenancePath,$HashManifestPath)){
 if(-not(Test-Path -LiteralPath $path -PathType Leaf)){Block 'SIGNING_BLOCKED' 'Unsigned artifact evidence missing'}
}
$unsignedHash=(Get-FileHash -LiteralPath $MsixPath -Algorithm SHA256).Hash.ToLowerInvariant()
$provenance=Get-Content -LiteralPath $ProvenancePath -Raw|ConvertFrom-Json
if($unsignedHash -cne $ExpectedUnsignedSha256.ToLowerInvariant()){Block 'SIGNING_BLOCKED' 'Unsigned MSIX hash mismatch'}
if($provenance.signature_status -ne 'UNSIGNED' -or $provenance.unsigned_msix_sha256 -cne $unsignedHash){Block 'SIGNING_BLOCKED' 'Unsigned provenance hash mismatch'}
if($provenance.source_sha -cne $ExpectedSourceSha -or $provenance.source_sha -notmatch '^[0-9a-f]{40}$'){Block 'SIGNING_BLOCKED' 'Artifact source SHA differs from workflow commit'}
$sumLine=Get-Content -LiteralPath $HashManifestPath|Where-Object{$_ -match [regex]::Escape((Split-Path $MsixPath -Leaf))}|Select-Object -First 1
if(-not $sumLine -or $sumLine -notmatch ('^'+[regex]::Escape($unsignedHash)+'\s')){Block 'SIGNING_BLOCKED' 'Unsigned SHA256SUMS entry mismatch'}
[xml]$manifest=Get-Content -LiteralPath $ManifestPath -Raw
$identity=$manifest.Package.Identity
if($identity.Publisher -cne $ExpectedPublisher -or $manifest.Package.Properties.PublisherDisplayName -cne $ExpectedPublisherDisplayName){Block 'SIGNING_BLOCKED' 'MSIX Publisher mismatch'}
if($identity.Name -cne 'TokenSaver.Phone02.Supervisor' -or $identity.ProcessorArchitecture -cne 'x64'){Block 'SIGNING_BLOCKED' 'Package Identity or architecture mismatch'}
$cert=Get-ChildItem -Path 'Cert:\LocalMachine\My'|Where-Object{$_.Thumbprint -ceq $ApprovedThumbprint.ToUpperInvariant()}|Select-Object -First 1
if($null -eq $cert){Block 'CERTIFICATE_PENDING' 'Approved certificate absent from LocalMachine\My'}
if($cert.Thumbprint -cne $ApprovedThumbprint.ToUpperInvariant() -or $cert.Subject -cne $ExpectedPublisher){Block 'SIGNING_BLOCKED' 'Certificate thumbprint or Subject mismatch'}
$now=[DateTime]::UtcNow
if($now -lt $cert.NotBefore.ToUniversalTime() -or $now -gt $cert.NotAfter.ToUniversalTime()){Block 'SIGNING_BLOCKED' 'Certificate outside validity period'}
if(-not $cert.HasPrivateKey){Block 'SIGNING_BLOCKED' 'Private key unavailable'}
$eku=@($cert.Extensions|Where-Object{$_ -is [System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]}|ForEach-Object{$_.EnhancedKeyUsages}|ForEach-Object{$_.Value})
if($eku -notcontains '1.3.6.1.5.5.7.3.3'){Block 'SIGNING_BLOCKED' 'Code Signing EKU missing'}
$chain=[System.Security.Cryptography.X509Certificates.X509Chain]::new()
$chain.ChainPolicy.RevocationMode=[System.Security.Cryptography.X509Certificates.X509RevocationMode]::Online
$chain.ChainPolicy.RevocationFlag=[System.Security.Cryptography.X509Certificates.X509RevocationFlag]::ExcludeRoot
if(-not $chain.Build($cert)){Block 'SIGNING_BLOCKED' 'Certificate chain/revocation validation failed'}
$pf=[Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFilesX86)
$sdk=Get-ChildItem (Join-Path $pf 'Windows Kits\10\bin') -Directory|Where-Object Name -Match '^10\.'|Sort-Object Name -Descending|Select-Object -First 1
$signtool=if($sdk){Join-Path $sdk.FullName 'x64\signtool.exe'}else{$null}
if(-not $signtool -or -not(Test-Path -LiteralPath $signtool)){Block 'SIGNING_BLOCKED' 'SignTool unavailable'}
if(-not $OutputDirectory){$OutputDirectory=Split-Path -Parent $MsixPath}
New-Item -ItemType Directory -Force -Path $OutputDirectory|Out-Null
$signedPath=Join-Path $OutputDirectory 'TokenSaver.Phone02.Supervisor.signed.msix'
Copy-Item -LiteralPath $MsixPath -Destination $signedPath -Force
# Certificate Store key access only; no PFX, password, /f or /p argument.
& $signtool sign /fd SHA256 /sha1 $cert.Thumbprint /s My /sm /tr $TimestampUrl /td SHA256 $signedPath
if($LASTEXITCODE -ne 0){Block 'SIGNING_BLOCKED' 'SignTool signing failed'}
$metadata=[ordered]@{
 source_sha=$provenance.source_sha
 unsigned_msix_sha256=$unsignedHash
 signed_msix_sha256=(Get-FileHash -LiteralPath $signedPath -Algorithm SHA256).Hash.ToLowerInvariant()
 package_identity=$identity.Name
 publisher=$ExpectedPublisher
 publisher_display_name=$ExpectedPublisherDisplayName
 certificate_thumbprint=$cert.Thumbprint
 certificate_subject=$cert.Subject
 certificate_not_before_utc=$cert.NotBefore.ToUniversalTime().ToString('o')
 certificate_not_after_utc=$cert.NotAfter.ToUniversalTime().ToString('o')
 certificate_chain_status='BUILT_AND_TRUSTED'
 tsa_url=$TimestampUrl
 timestamp_status='PENDING_CRYPTOGRAPHIC_VERIFY'
 signature_status='PENDING_VERIFICATION'
 verification_status='PENDING'
}
$metadata|ConvertTo-Json -Depth 8|Set-Content (Join-Path $OutputDirectory 'provenance-signed.json') -Encoding utf8
Write-Host "SOURCE_SHA=$($provenance.source_sha)"
Write-Host "UNSIGNED_MSIX_SHA256=$unsignedHash"
Write-Host "SIGNED_MSIX_SHA256=$($metadata.signed_msix_sha256)"
Write-Host "PUBLISHER=$ExpectedPublisher"
Write-Host "CERTIFICATE_THUMBPRINT=$($cert.Thumbprint)"
Write-Host 'SIGNATURE_STATUS=SIGNATURE_PENDING'
