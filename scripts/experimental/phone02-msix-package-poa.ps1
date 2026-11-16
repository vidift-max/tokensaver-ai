$ErrorActionPreference = 'Stop'
$root = Join-Path $env:RUNNER_TEMP 'phone02-msix-poa'
$pkg = Join-Path $root 'package'
New-Item -ItemType Directory -Force -Path $pkg | Out-Null
$log = Join-Path $root 'mission.log'
Start-Transcript -Path $log -Force | Out-Null
try {
  Write-Host 'ROUND_1 — runner, branch, baseline and isolation'
  $branch=(git branch --show-current).Trim(); $sha=(git rev-parse HEAD).Trim()
  $os=Get-CimInstance Win32_OperatingSystem
  if($branch -ne 'security/phone-02-windows-identity-poa'){throw "Wrong branch: $branch"}
  if($os.BuildNumber -ne '26100'){throw "Unexpected Windows build: $($os.BuildNumber)"}
  $sdkRoot=Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
  $sdkVersion='10.0.26100.0'
  $makeappx=Join-Path $sdkRoot "$sdkVersion\x64\MakeAppx.exe"
  $signtool=Join-Path $sdkRoot "$sdkVersion\x64\SignTool.exe"
  $vswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
  $vs=(& $vswhere -latest -products '*' -property installationPath).Trim()
  $vcvars=Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'
  $msbuild=Join-Path $vs 'MSBuild\Current\Bin\MSBuild.exe'
  [pscustomobject]@{Branch=$branch;SHA=$sha;Windows=$os.Caption;Version=$os.Version;Build=$os.BuildNumber;Architecture=$os.OSArchitecture;PowerShell=$PSVersionTable.PSVersion.ToString();Node=(& node --version);MakeAppx=$makeappx;SignTool=$signtool;VS=$vs;MSBuild=$msbuild;VCVars=$vcvars} | Format-List
  if(-not (Test-Path $makeappx) -or -not (Test-Path $signtool) -or -not (Test-Path $vcvars) -or -not (Test-Path $msbuild)){throw 'Required SDK/VS tool missing'}
  Write-Host 'ROUND_2 — package layout'
  $source=Join-Path $env:GITHUB_WORKSPACE 'scripts\experimental\phone02-msix-client.cpp'
  $exe=Join-Path $pkg 'client.exe'
  Write-Host 'ROUND_3 — generate AppxManifest'
  $publisher='CN=TokenSaver Phone02 PoA TEST ONLY'
  $manifest=@"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10" xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10" IgnorableNamespaces="uap uap10">
  <Identity Name="TokenSaver.Phone02.IdentityPoA" Publisher="$publisher" Version="1.0.0.0" ProcessorArchitecture="x64" />
  <Properties><DisplayName>TokenSaver Phone02 Identity PoA TEST ONLY</DisplayName><PublisherDisplayName>TEST ONLY</PublisherDisplayName><Logo>Assets/StoreLogo.png</Logo></Properties>
  <Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0" /></Dependencies>
  <Resources><Resource Language="en-us" /></Resources>
  <Applications><Application Id="App" Executable="client.exe" EntryPoint="Windows.PartialTrustApplication" uap10:TrustLevel="appContainer" uap10:RuntimeBehavior="packagedClassicApp"><uap:VisualElements DisplayName="TokenSaver Phone02 PoA" Description="Disposable identity test" BackgroundColor="transparent" Square150x150Logo="Assets/Square150x150Logo.png" Square44x44Logo="Assets/Square44x44Logo.png" /></Application></Applications>
  <Capabilities/>
</Package>
"@
  Set-Content -Path (Join-Path $pkg 'AppxManifest.xml') -Value $manifest -Encoding utf8
  New-Item -ItemType Directory -Force -Path (Join-Path $pkg 'Assets') | Out-Null
  Add-Type -AssemblyName System.Drawing
  foreach($name in @('StoreLogo.png','Square150x150Logo.png','Square44x44Logo.png')){ $bmp=New-Object System.Drawing.Bitmap 64,64; $bmp.Save((Join-Path $pkg "Assets\$name"),[System.Drawing.Imaging.ImageFormat]::Png);$bmp.Dispose() }
  Write-Host 'ROUND_4 — TEST-ONLY certificate and signature'
  $cert=New-SelfSignedCertificate -Type Custom -Subject $publisher -KeyUsage DigitalSignature -FriendlyName 'DISPOSABLE TEST ONLY — Phone02 MSIX PoA' -CertStoreLocation 'Cert:\CurrentUser\My' -KeyAlgorithm RSA -KeyLength 2048 -HashAlgorithm SHA256 -NotAfter (Get-Date).AddDays(2)
  $certPath=Join-Path $root 'test-only.cer'; Export-Certificate -Cert $cert -FilePath $certPath | Out-Null
  Import-Certificate -FilePath $certPath -CertStoreLocation 'Cert:\CurrentUser\TrustedPeople' | Out-Null
  Import-Certificate -FilePath $certPath -CertStoreLocation 'Cert:\CurrentUser\Root' | Out-Null
  Write-Host "CERT_SUBJECT=$($cert.Subject)";Write-Host "CERT_THUMBPRINT=$($cert.Thumbprint)"
  Write-Host 'ROUND_5 — compile and package'
  $buildCmd='"'+$vcvars+'" && cl /nologo /EHsc /W4 /DUNICODE /D_UNICODE "'+$source+'" /Fe:"'+$exe+'" advapi32.lib userenv.lib kernel32.lib'
  & cmd.exe /d /s /c $buildCmd
  if($LASTEXITCODE -ne 0){throw "C++ compile failed: $LASTEXITCODE"}
  $msix=Join-Path $root 'Phone02IdentityPoA.msix'
  & $makeappx pack /d $pkg /p $msix /o
  if($LASTEXITCODE -ne 0){throw "MakeAppx failed: $LASTEXITCODE"}
  & $signtool sign /fd SHA256 /sha1 $cert.Thumbprint $msix
  if($LASTEXITCODE -ne 0){throw "SignTool signing failed: $LASTEXITCODE"}
  & $signtool verify /pa /v $msix
  if($LASTEXITCODE -ne 0){throw "SignTool verification failed: $LASTEXITCODE"}
  Get-FileHash $msix -Algorithm SHA256 | Format-List
  Write-Host 'MSIX_CREATED'
  Write-Host 'ROUND_6 — install using Windows deployment API'
  Add-AppxPackage -Path $msix -ForceUpdateFromAnyVersion
  $p=Get-AppxPackage -Name 'TokenSaver.Phone02.IdentityPoA'
  if(-not $p){throw 'Windows package registration not found'}
  $p | Select-Object Name,PackageFullName,PackageFamilyName,Publisher,Version,InstallLocation | Format-List
  Write-Host 'MSIX_INSTALLED'
  Write-Host 'ROUND_7 — prepare Supervisor, then activate through AppsFolder package activation'
  $supervisor=Start-Process -FilePath $exe -ArgumentList @('--supervisor',$p.PackageFamilyName) -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $root 'supervisor.log') -RedirectStandardError (Join-Path $root 'supervisor.err')
  Start-Sleep -Milliseconds 700
  $aumid="$($p.PackageFamilyName)!App"
  $shell=New-Object -ComObject Shell.Application
  $shell.ShellExecute("shell:AppsFolder\$aumid",'','','open',1)
  Start-Sleep -Seconds 3
  $proc=Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq (Join-Path $p.InstallLocation 'client.exe') } | Select-Object -First 1
  if(-not $proc){Write-Warning 'Package activation was requested, but process PID was not independently found.'}
  else {Write-Host "MSIX_CLIENT_PID=$($proc.ProcessId)";Write-Host 'MSIX_CLIENT_ACTIVATED'}
  Write-Host 'ROUND_8 — token observation'
  Start-Sleep -Seconds 2
  Get-Content (Join-Path $root 'supervisor.log') -ErrorAction SilentlyContinue
  Write-Host 'ROUND_9 — independent package/process correlation'
  if($proc){Get-CimInstance Win32_Process -Filter "ProcessId=$($proc.ProcessId)" | Select-Object ProcessId,ExecutablePath,ParentProcessId | Format-List}
  Write-Host 'IDENTITY_EXPECTATION=Manifest requests packagedClassicApp with appContainer trust; actual token result is authoritative.'
  Write-Host 'ROUND_10 — report and cleanup'
  Write-Host 'MSIX_PACKAGE_SETUP_COMPLETED; full AppContainer identity PoA remains unproven.'
}
finally {
  Stop-Transcript | Out-Null
  Get-ChildItem $root -Force | Select-Object Name,Length | Format-Table
}
