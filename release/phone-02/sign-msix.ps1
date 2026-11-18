[CmdletBinding()]
param(
 [Parameter(Mandatory=$true)][string]$MsixPath,
 [Parameter(Mandatory=$true)][string]$ExpectedPublisher,
 [Parameter(Mandatory=$true)][string]$ExpectedPublisherDisplayName,
 [Parameter(Mandatory=$true)][string]$SecurityApprovalSecret,
 [Parameter(Mandatory=$true)][string]$CertificatePath,
 [Parameter(Mandatory=$true)][string]$CertificatePassword,
 [Parameter(Mandatory=$true)][string]$ManifestPath,
 [Parameter(Mandatory=$true)][string]$ApprovedThumbprint,
 [Parameter(Mandatory=$true)][string]$ProvenancePath,
 [Parameter(Mandatory=$true)][string]$HashManifestPath,
 [string]$TimestampUrl='http://timestamp.digicert.com'
)
$ErrorActionPreference='Stop'
if($ExpectedPublisher -match 'PENDING APPROVAL|TEST ONLY'){throw 'Signing denied: Publisher is not approved'}
if($ExpectedPublisherDisplayName -match 'PENDING APPROVAL|TEST ONLY'){throw 'Signing denied: Publisher display name is not approved'}
if($SecurityApprovalSecret -ne 'APPROVED'){throw 'Signing denied: explicit Security approval secret is missing or invalid'}
if(-not(Test-Path $MsixPath)){throw 'MSIX input missing'}
if(-not(Test-Path $CertificatePath)){throw 'Certificate input missing'}
$cert=[System.Security.Cryptography.X509Certificates.X509Certificate2]::new($CertificatePath,$CertificatePassword,[System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet)
if($cert.Subject -ne $ExpectedPublisher){throw 'Certificate subject does not match approved Publisher'}
if(-not $cert.HasPrivateKey){throw 'Signing certificate has no private key'}
if($cert.Thumbprint -ne $ApprovedThumbprint){throw 'Certificate fingerprint does not match separately approved fingerprint'}
if(-not(Test-Path $ManifestPath)){throw 'Package manifest evidence missing'}
$manifest=[xml](Get-Content $ManifestPath -Raw)
if($manifest.Package.Identity.Publisher -ne $ExpectedPublisher){throw 'Package manifest Publisher mismatch'}
if($manifest.Package.Identity.Name -ne 'TokenSaver.Phone02.Supervisor'){throw 'Package Identity name mismatch'}
if($manifest.Package.Identity.ProcessorArchitecture -ne 'x64'){throw 'Package architecture mismatch'}
if($manifest.Package.Identity.Version -notmatch '^\d+\.\d+\.\d+\.\d+$'){throw 'Package version invalid'}
if(-not(Test-Path $ProvenancePath)){throw 'Provenance evidence missing before signing'}
$preSignProvenance=Get-Content $ProvenancePath -Raw|ConvertFrom-Json
$preSignHash=(Get-FileHash $MsixPath -Algorithm SHA256).Hash.ToLower()
if($preSignProvenance.unsigned_msix_sha256 -ne $preSignHash){throw 'Unsigned package hash does not match provenance'}
if($preSignProvenance.source_sha -notmatch '^[0-9a-f]{40}$'){throw 'Provenance source SHA missing or invalid'}
if($preSignProvenance.package_identity -ne $manifest.Package.Identity.Name -or $preSignProvenance.publisher -ne $ExpectedPublisher -or $preSignProvenance.publisher_display_name -ne $ExpectedPublisherDisplayName -or $preSignProvenance.version -ne $manifest.Package.Identity.Version){throw 'Manifest fields do not match provenance record'}
if($manifest.Package.Properties.PublisherDisplayName -ne $ExpectedPublisherDisplayName){throw 'Package manifest PublisherDisplayName mismatch'}
$sdk=Get-ChildItem (Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin') -Directory|Where-Object Name -Match '^10\.'|Sort-Object Name -Descending|Select-Object -First 1
$signtool=Join-Path $sdk.FullName 'x64\SignTool.exe';if(-not(Test-Path $signtool)){throw 'SignTool unavailable'}
& $signtool sign /fd SHA256 /f $CertificatePath /p $CertificatePassword /tr $TimestampUrl /td SHA256 $MsixPath
if($LASTEXITCODE -ne 0){throw 'SignTool signing failed'}
$verifyOutput = & $signtool verify /pa /v $MsixPath 2>&1
if($LASTEXITCODE -ne 0){throw 'SignTool verification failed'}
$verifyText=($verifyOutput -join ' ')
if($verifyText -notmatch '(?i)timestamp|time stamp'){throw 'Timestamp verification evidence missing'}
$signature=Get-AuthenticodeSignature $MsixPath
if($signature.Status -ne 'Valid'){throw 'Signed package signature status is not Valid'}
Write-Host ('SIGNED_MSIX_SHA256='+(Get-FileHash $MsixPath -Algorithm SHA256).Hash.ToLower())
Write-Host ('SIGNER_SUBJECT='+$cert.Subject)
Write-Host ('SIGNER_THUMBPRINT='+$cert.Thumbprint)
Write-Host ('TIMESTAMP_URL='+$TimestampUrl)
Write-Host 'SIGNATURE_VALID=PASS'
if(Test-Path $ProvenancePath){
 $p=[ordered]@{}; (Get-Content $ProvenancePath -Raw|ConvertFrom-Json -AsHashtable).GetEnumerator()|ForEach-Object{$p[$_.Key]=$_.Value}
 $p['signed_msix_sha256']=(Get-FileHash $MsixPath -Algorithm SHA256).Hash.ToLower();$p['signature_status']='VALID';$p['certificate_fingerprint']=$cert.Thumbprint;$p['timestamp_status']='SIGNTOOL_VERIFY_OUTPUT_TIMESTAMP_PRESENT';$p['signing_approval_status']='CERTIFICATE_FINGERPRINT_MATCH';$p['security_approval_status']='SECURITY_APPROVAL_SECRET_PRESENT; GITHUB_ENVIRONMENT_PROTECTION_POLICY_NOT_VERIFIED';$p['status']='SIGNED_AND_VERIFIED';$p|ConvertTo-Json -Depth 10|Set-Content $ProvenancePath -Encoding utf8
}
if(Test-Path $HashManifestPath){
 $root=Split-Path (Split-Path $HashManifestPath -Parent) -Parent
 $entries=Get-ChildItem $root -File -Recurse | Where-Object FullName -ne $HashManifestPath | ForEach-Object{(Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()+'  '+([IO.Path]::GetRelativePath($root,$_.FullName))}
 $entries|Set-Content $HashManifestPath -Encoding ascii
}
