[CmdletBinding()]
param(
  [Parameter(Mandatory=$true)][string]$ExpectedSourceSha,
  [string]$Publisher = 'CN=TokenSaver Publisher',
  [string]$PublisherDisplayName = 'TokenSaver AI',
  [string]$PackageVersion = '0.1.0.0',
  [string]$OutputDirectory = ''
)
$ErrorActionPreference = 'Stop'
function Assert-Check([bool]$Condition,[string]$Name){ if(-not $Condition){ throw "RELEASE_GATE_FAIL=$Name" }; Write-Host "RELEASE_GATE_PASS=$Name" }
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
Set-Location $repo
$actualSha = (& git rev-parse HEAD).Trim()
Assert-Check ($actualSha -eq $ExpectedSourceSha) 'SOURCE_SHA_EXACT'
$dirty=(& git status --porcelain | Out-String).Trim()
Assert-Check ([string]::IsNullOrWhiteSpace($dirty)) 'WORKTREE_CLEAN'
Assert-Check ($Publisher -match '^CN=TokenSaver Publisher PENDING APPROVAL$' -or $Publisher -match '^CN=') 'PUBLISHER_FORMAT'
Assert-Check ($PackageVersion -match '^(0|[1-9]\d{0,3})\.(0|[1-9]\d{0,3})\.(0|[1-9]\d{0,3})\.(0|[1-9]\d{0,3})$') 'VERSION_FORMAT'
if(-not $OutputDirectory){ $OutputDirectory=Join-Path $env:RUNNER_TEMP 'phone02-release-package' }
$stage=Join-Path $OutputDirectory 'package';$build=Join-Path $OutputDirectory 'build';$evidence=Join-Path $OutputDirectory 'evidence'
New-Item -ItemType Directory -Force -Path $stage,$build,$evidence | Out-Null
Start-Transcript -Path (Join-Path $evidence 'build-transcript.txt') -Force | Out-Null
$vswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
Assert-Check (Test-Path $vswhere) 'VSWHERE_PRESENT'
$vs=(& $vswhere -latest -products '*' -property installationPath).Trim()
Assert-Check ($vs.Length -gt 0) 'VS_INSTALLATION_FOUND'
$vcvars=Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'
Assert-Check (Test-Path $vcvars) 'VCVARS_X64_PRESENT'
$kits=Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$sdk=Get-ChildItem $kits -Directory | Where-Object Name -Match '^10\.' | Sort-Object Name -Descending | Select-Object -First 1
Assert-Check ($null -ne $sdk) 'WINDOWS_SDK_FOUND'
$midl=Join-Path $sdk.FullName 'x64\midl.exe';$makeappx=Join-Path $sdk.FullName 'x64\MakeAppx.exe'
Assert-Check ((Test-Path $midl) -and (Test-Path $makeappx)) 'MIDL_MAKEAPPX_PRESENT'
$native=Join-Path $repo 'services\phone-supervisor';$idl=Join-Path $native 'phone_supervisor_rpc.idl'
$midlOutput = (& $midl /? 2>&1 | Out-String)
$midlVersion = (($midlOutput -split "`r?`n" | Where-Object { $_ -match '(?i)MIDL.*Compiler Version' } | Select-Object -First 1) -join '').Trim()
$msvcRoot=Join-Path $vs 'VC\Tools\MSVC'
$msvcToolset=Get-ChildItem $msvcRoot -Directory | Sort-Object Name -Descending | Select-Object -First 1
$clExe=if($msvcToolset){Join-Path $msvcToolset.FullName 'bin\Hostx64\x64\cl.exe'}else{''}
Assert-Check ($clExe -and (Test-Path $clExe)) 'MSVC_COMPILER_PRESENT'
$compilerOutput = (& $clExe 2>&1 | Out-String)
$compilerVersion = (($compilerOutput -split "`r?`n" | Where-Object { $_ -match '(?i)Compiler Version' } | Select-Object -First 1) -join '').Trim()
Assert-Check ($compilerVersion -match '(?i)Compiler Version') 'COMPILER_VERSION_CAPTURED'
Assert-Check ($midlVersion -match 'MIDL Compiler Version') 'MIDL_VERSION_CAPTURED'
$cmdMidl='"'+$vcvars+'" && "'+$midl+'" /nologo /env x64 /h "'+(Join-Path $build 'phone_supervisor_rpc.h')+'" /iid "'+(Join-Path $build 'phone_supervisor_rpc_i.c')+'" /out "'+$build+'" "'+$idl+'"'
& cmd.exe /d /s /c $cmdMidl
Assert-Check ($LASTEXITCODE -eq 0) 'MIDL_GENERATION'
$rpcStubFiles=@((Join-Path $build 'phone_supervisor_rpc.h'),(Join-Path $build 'phone_supervisor_rpc_c.c'),(Join-Path $build 'phone_supervisor_rpc_s.c'))
Assert-Check (($rpcStubFiles | Where-Object { -not (Test-Path $_) }).Count -eq 0) 'MIDL_RPC_STUBS_PRESENT'
$supervisor=Join-Path $stage 'Supervisor.exe';$client=Join-Path $stage 'Phone02Client.exe';$hostClient=Join-Path $stage 'SupervisorHost.exe'
$cmdSvc='"'+$vcvars+'" && cl /nologo /EHsc /W4 /std:c++20 /MT /DUNICODE /D_UNICODE /I"'+$build+'" "'+(Join-Path $native 'phone_supervisor.cpp')+'" "'+(Join-Path $native 'RealSupervisorTeamRoomHttpsAdapter.cpp')+'" "'+(Join-Path $native 'SupervisorCredentialStoreR5.cpp')+'" "'+(Join-Path $native 'SupervisorCredentialProvisioningR5.cpp')+'" "'+(Join-Path $build 'phone_supervisor_rpc_s.c')+'" /Fe:"'+$supervisor+'" advapi32.lib rpcrt4.lib bcrypt.lib ncrypt.lib userenv.lib shell32.lib ole32.lib winhttp.lib'
& cmd.exe /d /s /c $cmdSvc
Assert-Check ($LASTEXITCODE -eq 0) 'SUPERVISOR_COMPILE'
$cmdCli='"'+$vcvars+'" && cl /nologo /EHsc /W4 /std:c++20 /MT /DUNICODE /D_UNICODE /I"'+$build+'" "'+(Join-Path $native 'phone_supervisor_client.cpp')+'" "'+(Join-Path $build 'phone_supervisor_rpc_c.c')+'" /Fe:"'+$client+'" rpcrt4.lib ole32.lib'
& cmd.exe /d /s /c $cmdCli
Assert-Check ($LASTEXITCODE -eq 0) 'CLIENT_COMPILE'
Copy-Item $client $hostClient
$assets=Join-Path $stage 'Assets';New-Item -ItemType Directory -Force $assets | Out-Null
Add-Type -AssemblyName System.Drawing
$sourceIcon=Join-Path $repo 'src-tauri\icons\icon.png'
Assert-Check (Test-Path $sourceIcon) 'ICON_SOURCE_PRESENT'
foreach($item in @(@('StoreLogo.png',50),@('Square150x150Logo.png',150),@('Square44x44Logo.png',44))){
  $img=[System.Drawing.Image]::FromFile($sourceIcon);$bmp=New-Object System.Drawing.Bitmap $item[1],$item[1]
  $g=[System.Drawing.Graphics]::FromImage($bmp);$g.DrawImage($img,0,0,$item[1],$item[1]);$bmp.Save((Join-Path $assets $item[0]),[System.Drawing.Imaging.ImageFormat]::Png);$g.Dispose();$bmp.Dispose();$img.Dispose()
}
$template=Get-Content (Join-Path $PSScriptRoot 'AppxManifest.template.xml') -Raw
$publisherXml=[System.Security.SecurityElement]::Escape($Publisher)
$displayNameXml=[System.Security.SecurityElement]::Escape($PublisherDisplayName)
$manifest=$template.Replace('__PUBLISHER__',$publisherXml).Replace('__PUBLISHER_DISPLAY_NAME__',$displayNameXml).Replace('__VERSION__',$PackageVersion)
$manifestPath=Join-Path $stage 'AppxManifest.xml';Set-Content -Path $manifestPath -Value $manifest -Encoding utf8
[xml]$xml=$manifest
$identity=$xml.Package.Identity
Assert-Check ($identity.Name -eq 'TokenSaver.Phone02.Supervisor') 'PACKAGE_IDENTITY'
Assert-Check ($identity.ProcessorArchitecture -eq 'x64') 'PACKAGE_ARCH_X64'
Assert-Check ($identity.Publisher -eq $Publisher) 'PUBLISHER_MANIFEST_MATCH'
Assert-Check ($xml.Package.Properties.PublisherDisplayName -eq $PublisherDisplayName) 'PUBLISHER_DISPLAY_NAME_MATCH'
Assert-Check ($xml.Package.Applications.Application.Executable -contains 'Phone02Client.exe') 'CLIENT_ENTRYPOINT'
Assert-Check ($manifest -match 'Supervisor\.exe' -and $manifest -match 'Windows\.FullTrustApplication') 'SUPERVISOR_FULLTRUST_EXTENSION'
Assert-Check ($manifest -match 'runFullTrust') 'RUN_FULLTRUST_CAPABILITY'
Assert-Check ($manifest -notmatch 'TEST ONLY|SupervisorTest|P236|P238|fake|fixture') 'NO_TEST_MARKERS'
$msix=Join-Path $OutputDirectory 'TokenSaver.Phone02.Supervisor.unsigned.msix'
& $makeappx pack /d $stage /p $msix /o
Assert-Check ($LASTEXITCODE -eq 0 -and (Test-Path $msix)) 'MAKEAPPX_PACK'
$validation=Join-Path $OutputDirectory 'validation'
& $makeappx unpack /p $msix /d $validation
Assert-Check ($LASTEXITCODE -eq 0 -and (Test-Path (Join-Path $validation 'AppxManifest.xml'))) 'MAKEAPPX_VALIDATE'
$sha=(Get-FileHash $msix -Algorithm SHA256).Hash.ToLower()
$assetFiles=Get-ChildItem $assets -File
$files=@($msix,$supervisor,$client,$hostClient,$manifestPath)+$assetFiles.FullName+$rpcStubFiles | ForEach-Object { $f=Get-Item $_;[ordered]@{name=$f.Name;path=[IO.Path]::GetRelativePath($OutputDirectory,$f.FullName).Replace('\','/');sha256=(Get-FileHash $f.FullName -Algorithm SHA256).Hash.ToLower();bytes=$f.Length} }
$provenance=[ordered]@{
  release_id=('phone02-'+$actualSha.Substring(0,12)+'-run-'+$env:GITHUB_RUN_ID)
  repository=$env:GITHUB_REPOSITORY;source_sha=$actualSha;source_branch=$(if($env:GITHUB_REF_NAME){$env:GITHUB_REF_NAME}else{'DETACHED_HEAD'})
  package_identity=$identity.Name;publisher=$Publisher;publisher_display_name=$PublisherDisplayName;version=$PackageVersion;architecture='x64';target_device_family='Windows.Desktop';
  workflow=$env:GITHUB_WORKFLOW;workflow_run_id=$env:GITHUB_RUN_ID;workflow_run_attempt=$env:GITHUB_RUN_ATTEMPT;job=$env:GITHUB_JOB
  toolchain=[ordered]@{runner_image='windows-2025 (mutable hosted-image label)';toolchain_pin_status='EXACT_VERSIONS_RECORDED_AT_BUILD; IMAGE_DIGEST_NOT_PINNED';visual_studio_path=$vs;windows_sdk_version=$sdk.Name;midl_path=$midl;midl_version=$midlVersion;compiler='MSVC x64';compiler_version=$compilerVersion;makeappx_path=$makeappx;build_flags='/EHsc /W4 /std:c++20 /MT /DUNICODE /D_UNICODE'}
  reproducibility_status='SOURCE_SHA_AND_BUILD_RECIPE_CAPTURED; BYTE_IDENTICAL_REBUILD_NOT_VERIFIED'
  manifest_sha256=(Get-FileHash $manifestPath -Algorithm SHA256).Hash.ToLower();unsigned_msix_sha256=$sha;signature_status='UNSIGNED';certificate_fingerprint=$null;timestamp_status='NOT_APPLICABLE_UNSIGNED';approval_status='PENDING';artifacts=$files;status='UNSIGNED_PACKAGE_STRUCTURALLY_BUILT'
}
$provenanceFile=Join-Path $evidence 'provenance.json';$provenance|ConvertTo-Json -Depth 10|Set-Content $provenanceFile -Encoding utf8
$buildSummary=Join-Path $evidence 'build-summary.txt'
$summaryLines=@(
  'PHASE_BUILD=PASS'
  'PHASE_PACKAGE=PASS'
  'PHASE_HASH=PASS'
  'PACKAGE_VALIDATION=MAKEAPPX_UNPACK_AND_MANIFEST_PASS'
  'PHASE_SIGN=NOT_RUN_UNSIGNED'
  "SOURCE_SHA=$actualSha"
  "PACKAGE_IDENTITY=$($identity.Name)"
  "PUBLISHER=$Publisher"
  "PUBLISHER_DISPLAY_NAME=$PublisherDisplayName"
  "VERSION=$PackageVersion"
  'ARCH=x64'
  'TARGET=Windows.Desktop'
  "MSIX_SHA256=$sha"
  'PROVENANCE=provenance.json'
)
$summaryLines|Set-Content $buildSummary -Encoding utf8
$hashRows=@($msix,$supervisor,$client,$hostClient,$manifestPath,$provenanceFile)+$assetFiles.FullName+$rpcStubFiles|ForEach-Object{
  $relative=[IO.Path]::GetRelativePath($OutputDirectory,$_.ToString()).Replace('\','/')
  (Get-FileHash $_ -Algorithm SHA256).Hash.ToLower()+'  '+$relative
}
$hashRows|Set-Content (Join-Path $evidence 'SHA256SUMS') -Encoding ascii
Write-Host "PACKAGE_STATUS=UNSIGNED"
Write-Host "PACKAGE_PATH=$msix"
Write-Host "PACKAGE_SHA256=$sha"
Write-Host "SOURCE_SHA=$actualSha"
Write-Host "PUBLISHER=$Publisher"
Write-Host "PROVENANCE_PATH=$provenanceFile"
Write-Host 'SIGNING_STATUS=SIGNING READY — CERTIFICATE/SIGNER PENDING'
Stop-Transcript | Out-Null
