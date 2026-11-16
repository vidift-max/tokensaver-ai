$ErrorActionPreference = 'Stop'
$root = Join-Path $env:RUNNER_TEMP 'phone02-identity-controls-52881'
$pkg = Join-Path $root 'package'
New-Item -ItemType Directory -Force -Path $pkg | Out-Null
$log = Join-Path $root 'mission-52881.log'
Start-Transcript -Path $log -Force | Out-Null
$cert=$null; $package=$null
try {
  Write-Host 'ROUND_1 — prior run state and immutable evidence boundary'
  Write-Host 'PRIOR_RUN=43817698805; last observed IN_PROGRESS; verify its final state independently.'
  $branch=(git branch --show-current).Trim(); $sha=(git rev-parse HEAD).Trim()
  if($branch -ne 'security/phone-02-windows-identity-poa'){throw "Wrong branch: $branch"}
  Write-Host "BRANCH=$branch"; Write-Host "SHA=$sha"
  Write-Host 'ROUND_2 — source audit and experimental-only scope'
  $source=Join-Path $env:GITHUB_WORKSPACE 'scripts\experimental\phone02-msix-client.cpp'
  if(-not (Test-Path $source)){throw 'Experimental C++ source missing'}
  Write-Host 'AUDIT=GetNamedPipeClientProcessId + impersonated pipe token + process token independently queried by PID.'
  Write-Host 'ROUND_3 — compile packaged client, Supervisor and unpackaged adversary'
  $sdkRoot=Join-Path \${env:ProgramFiles(x86)} 'Windows Kits\10\bin'; $sdkVersion='10.0.26100.0'
  $makeappx=Join-Path $sdkRoot "$sdkVersion\x64\MakeAppx.exe"; $signtool=Join-Path $sdkRoot "$sdkVersion\x64\SignTool.exe"
  $vswhere=Join-Path \${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
  $vs=(& $vswhere -latest -products '*' -property installationPath).Trim(); $vcvars=Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'
  if(-not (Test-Path $makeappx) -or -not (Test-Path $signtool) -or -not (Test-Path $vcvars)){throw 'Required Windows SDK/VS tool missing'}
  $harness=Join-Path $root 'phone02-harness.exe'
  $cmd='"'+$vcvars+'" && cl /nologo /EHsc /W4 /DUNICODE /D_UNICODE "'+$source+'" /Fe:"'+$harness+'" advapi32.lib userenv.lib kernel32.lib'
  & cmd.exe /d /s /c $cmd
  if($LASTEXITCODE -ne 0){throw "C++ compile failed: $LASTEXITCODE"}
  Copy-Item $harness (Join-Path $root 'supervisor.exe'); Copy-Item $harness (Join-Path $root 'common.exe'); Copy-Item $harness (Join-Path $pkg 'client.exe')
  Write-Host 'ROUND_4 — DACL policy; pipe reachability is not authorization'
  Write-Host 'DACL_POLICY=User SID + expected AppContainer SID; same-user process may connect, then must be rejected by OS-observed token.'
  Write-Host 'DACL_SAME_USER_LIMITATION=TRUE'
  Write-Host 'ROUND_5 — disposable MSIX, expected identity kept separate from observed identity'
  $publisher='CN=TokenSaver Phone02 PoA TEST ONLY'
  $manifest=@"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10" xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10" IgnorableNamespaces="uap uap10">
<Identity Name="TokenSaver.Phone02.IdentityPoA" Publisher="$publisher" Version="1.0.0.0" ProcessorArchitecture="x64"/>
<Properties><DisplayName>Phone02 Identity PoA TEST ONLY</DisplayName><PublisherDisplayName>TEST ONLY</PublisherDisplayName><Logo>Assets/StoreLogo.png</Logo></Properties>
<Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0"/></Dependencies>
<Resources><Resource Language="en-us"/></Resources>
<Applications><Application Id="App" Executable="client.exe" EntryPoint="Windows.PartialTrustApplication" uap10:TrustLevel="appContainer" uap10:RuntimeBehavior="packagedClassicApp"><uap:VisualElements DisplayName="Phone02 Identity PoA" Description="Disposable identity test" BackgroundColor="transparent" Square150x150Logo="Assets/Square150x150Logo.png" Square44x44Logo="Assets/Square44x44Logo.png"/></Application></Applications>
<Capabilities/>
</Package>
"@
  Set-Content (Join-Path $pkg 'AppxManifest.xml') $manifest -Encoding utf8
  New-Item -ItemType Directory -Force (Join-Path $pkg 'Assets') | Out-Null
  Add-Type -AssemblyName System.Drawing
  foreach($name in @('StoreLogo.png','Square150x150Logo.png','Square44x44Logo.png')){$bmp=New-Object System.Drawing.Bitmap 64,64;$bmp.Save((Join-Path $pkg "Assets\$name"),[System.Drawing.Imaging.ImageFormat]::Png);$bmp.Dispose()}
  $cert=New-SelfSignedCertificate -Type Custom -Subject $publisher -KeyUsage DigitalSignature -FriendlyName 'DISPOSABLE TEST ONLY — Phone02 PoA 52881' -CertStoreLocation 'Cert:\CurrentUser\My' -KeyAlgorithm RSA -KeyLength 2048 -HashAlgorithm SHA256 -NotAfter (Get-Date).AddDays(2)
  $certPath=Join-Path $root 'test-only.cer'; Export-Certificate -Cert $cert -FilePath $certPath | Out-Null
  Import-Certificate -FilePath $certPath -CertStoreLocation 'Cert:\CurrentUser\TrustedPeople' | Out-Null
  Import-Certificate -FilePath $certPath -CertStoreLocation 'Cert:\CurrentUser\Root' | Out-Null
  $msix=Join-Path $root 'Phone02IdentityPoA-52881.msix'
  & $makeappx pack /d $pkg /p $msix /o
  if($LASTEXITCODE -ne 0){throw "MakeAppx failed: $LASTEXITCODE"}
  & $signtool sign /fd SHA256 /sha1 $cert.Thumbprint $msix
  if($LASTEXITCODE -ne 0){throw "SignTool signing failed: $LASTEXITCODE"}
  & $signtool verify /pa /v $msix
  if($LASTEXITCODE -ne 0){throw "SignTool verification failed: $LASTEXITCODE"}
  Get-FileHash $msix -Algorithm SHA256 | Format-List
  Write-Host 'SIGNATURE_CREATED=TRUE'; Write-Host 'SIGNATURE_VALIDATED=TRUE'
  Write-Host "CERT_SUBJECT=$($cert.Subject)"; Write-Host "CERT_THUMBPRINT=$($cert.Thumbprint)"
  Add-AppxPackage -Path $msix -ForceUpdateFromAnyVersion
  $package=Get-AppxPackage -Name 'TokenSaver.Phone02.IdentityPoA'
  if(-not $package){throw 'Windows package registration not found'}
  $package | Select-Object Name,PackageFullName,PackageFamilyName,Publisher,Version,InstallLocation | Format-List
  Write-Host 'MSIX_INSTALLED=TRUE'
  Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class Phone02SidApi { [DllImport("userenv.dll", CharSet=CharSet.Unicode)] public static extern int DeriveAppContainerSidFromAppContainerName(string n, out IntPtr sid); [DllImport("advapi32.dll", CharSet=CharSet.Unicode)] public static extern bool ConvertSidToStringSid(IntPtr sid, out IntPtr text); [DllImport("kernel32.dll")] public static extern IntPtr LocalFree(IntPtr h); }' -PassThru | Out-Null
  $sidPtr=[IntPtr]::Zero; $rc=[Phone02SidApi]::DeriveAppContainerSidFromAppContainerName($package.PackageFamilyName,[ref]$sidPtr)
  if($rc -ne 0 -or $sidPtr -eq [IntPtr]::Zero){throw "Expected SID derivation failed: $rc"}
  $textPtr=[IntPtr]::Zero
  if(-not [Phone02SidApi]::ConvertSidToStringSid($sidPtr,[ref]$textPtr)){throw 'ConvertSidToStringSid failed'}
  $expectedSid=[Runtime.InteropServices.Marshal]::PtrToStringUni($textPtr)
  [Phone02SidApi]::LocalFree($textPtr) | Out-Null; [Phone02SidApi]::LocalFree($sidPtr) | Out-Null
  Write-Host "EXPECTED_APPCONTAINER_SID=$expectedSid SOURCE=DeriveAppContainerSidFromAppContainerName; comparison value only"
  $aumid="$($package.PackageFamilyName)!App"; $shell=New-Object -ComObject Shell.Application
  function Start-Supervisor([string]$caseName){
    $out=Join-Path $root "$caseName.supervisor.log";$err=Join-Path $root "$caseName.supervisor.err"
    $sp=Start-Process -FilePath (Join-Path $root 'supervisor.exe') -ArgumentList @('--supervisor',$package.PackageFamilyName,$expectedSid,$package.PackageFullName,$caseName) -PassThru -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError $err
    Start-Sleep -Milliseconds 800
    return @{Process=$sp;Out=$out;Err=$err}
  }
  function Finish-Case($s,[string]$caseName){
    if(-not $s.Process.WaitForExit(15000)){Stop-Process -Id $s.Process.Id -Force;throw "Supervisor timeout: $caseName"}
    $content=Get-Content $s.Out -Raw -ErrorAction SilentlyContinue
    Write-Host "CASE_BEGIN=$caseName";Write-Host $content;Write-Host "SUPERVISOR_EXIT=$($s.Process.ExitCode)";Write-Host "CASE_END=$caseName"
    return @{Content=$content;ExitCode=$s.Process.ExitCode}
  }
  Write-Host 'ROUND_6 — legitimate MSIX client and same-user ordinary process'
  $s=Start-Supervisor 'legitimate-msix'
  $shell.ShellExecute("shell:AppsFolder\$aumid","$($package.PackageFamilyName) $expectedSid",'','open',1)
  $legit=Finish-Case $s 'legitimate-msix'
  if($legit.ExitCode -ne 0 -or $legit.Content -notmatch 'AUTHORIZATION_RESULT=ALLOW'){throw 'Legitimate packaged client failed authorization; stop adversarial interpretation.'}
  Write-Host 'ROUND_7 — protocol spoof matrix'
  $modes=@('common','spoof-package','spoof-appcontainer','spoof-package-sid','spoof-handshake','spoof-all')
  foreach($mode in $modes){
    $s=Start-Supervisor $mode
    $cp=Start-Process -FilePath (Join-Path $root 'common.exe') -ArgumentList @('--adversary',$mode,$package.PackageFamilyName,$expectedSid) -PassThru -WindowStyle Hidden
    $result=Finish-Case $s $mode
    if($result.ExitCode -eq 0 -or $result.Content -match 'AUTHORIZATION_RESULT=ALLOW'){throw "FAIL: unauthorized common process allowed in case $mode"}
    if($result.Content -notmatch 'TOKEN_IS_APPCONTAINER=0'){throw "Common process token not proven non-AppContainer in case $mode"}
    Write-Host "COMMON_PROCESS_REJECTED=$mode"
  }
  Write-Host 'ROUND_8 — two package activations and instance identity assessment'
  $instancePids=@()
  foreach($i in 1..2){
    $s=Start-Supervisor "instance-$i"
    $shell.ShellExecute("shell:AppsFolder\$aumid","$($package.PackageFamilyName) $expectedSid",'','open',1)
    $result=Finish-Case $s "instance-$i"
    if($result.Content -match 'PIPE_CLIENT_PID=(\d+)'){$instancePids += [int]$Matches[1]}
    if($result.ExitCode -ne 0){throw "Legitimate activation failed during instance test $i"}
  }
  Write-Host "INSTANCE_PIDS=$($instancePids -join ',')"
  Write-Host 'NO_UNIQUE_INSTANCE_IDENTITY=TRUE — package/AppContainer identity is shared, not a per-process credential.'
  Write-Host 'ROUND_9 — supervisor closure, reconnect and PID/token revalidation'
  $noServer=Start-Process -FilePath (Join-Path $root 'common.exe') -ArgumentList @('--adversary','spoof-all',$package.PackageFamilyName,$expectedSid) -PassThru -Wait
  Write-Host "RECONNECT_WITHOUT_SUPERVISOR_EXIT=$($noServer.ExitCode) (nonzero expected)"
  $s=Start-Supervisor 'reconnect-common'
  $common=Start-Process -FilePath (Join-Path $root 'common.exe') -ArgumentList @('--adversary','spoof-all',$package.PackageFamilyName,$expectedSid) -PassThru
  $rejected=Finish-Case $s 'reconnect-common'
  if($rejected.ExitCode -eq 0 -or $rejected.Content -match 'AUTHORIZATION_RESULT=ALLOW'){throw 'FAIL: common process authorized after reconnect'}
  $s=Start-Supervisor 'reconnect-legitimate'
  $shell.ShellExecute("shell:AppsFolder\$aumid","$($package.PackageFamilyName) $expectedSid",'','open',1)
  $reconnected=Finish-Case $s 'reconnect-legitimate'
  if($reconnected.ExitCode -ne 0){throw 'Legitimate reconnect failed'}
  Write-Host 'ROUND_10 — summary and disposable cleanup'
  Write-Host "LEGITIMATE_ALLOW=$($legit.ExitCode -eq 0)"
  Write-Host "COMMON_REJECT=$($rejected.ExitCode -ne 0)"
  Write-Host "RECONNECT_LEGITIMATE_ALLOW=$($reconnected.ExitCode -eq 0)"
  Write-Host 'POA_RESULT=PARTIAL_PENDING_INDEPENDENT_REVIEW'
}
finally {
  if($package){Remove-AppxPackage -Package $package.PackageFullName -ErrorAction SilentlyContinue}
  if($cert){
    Remove-Item "Cert:\CurrentUser\My\$($cert.Thumbprint)" -ErrorAction SilentlyContinue
    Remove-Item "Cert:\CurrentUser\Root\$($cert.Thumbprint)" -ErrorAction SilentlyContinue
    Remove-Item "Cert:\CurrentUser\TrustedPeople\$($cert.Thumbprint)" -ErrorAction SilentlyContinue
  }
  Stop-Transcript | Out-Null
  Get-ChildItem $root -Force | Select-Object Name,Length | Format-Table
}