$ErrorActionPreference = 'Stop'
$root = Join-Path $env:RUNNER_TEMP 'phone02-identity-controls-52881'
$pkg = Join-Path $root 'package'
New-Item -ItemType Directory -Force -Path $pkg | Out-Null
$log = Join-Path $root 'mission-52881.log'
Start-Transcript -Path $log -Force | Out-Null
function Invoke-BoundedTool([string]$FilePath,[string]$Arguments,[string]$Label,[int]$TimeoutSeconds=120) {
  $safe=($Label -replace '[^A-Za-z0-9_-]','_')
  $stdout=Join-Path $root "$safe.stdout.log"; $stderr=Join-Path $root "$safe.stderr.log"
  Write-Host "PROCESS_BEGIN=$Label FILE=$FilePath ARGUMENTS=$Arguments TIMEOUT_SECONDS=$TimeoutSeconds"
  $proc=Start-Process -FilePath $FilePath -ArgumentList $Arguments -PassThru -NoNewWindow -RedirectStandardOutput $stdout -RedirectStandardError $stderr
  if(-not $proc.WaitForExit($TimeoutSeconds*1000)) {
    try { $proc.Kill() } catch {}
    Write-Host "PROCESS_TIMEOUT=$Label PID=$($proc.Id)"
    if(Test-Path $stdout){Get-Content $stdout -Raw | Write-Host}
    if(Test-Path $stderr){Get-Content $stderr -Raw | Write-Host}
    throw "Timed out after $TimeoutSeconds seconds: $Label"
  }
  if(Test-Path $stdout){Get-Content $stdout -Raw | Write-Host}
  if(Test-Path $stderr){Get-Content $stderr -Raw | Write-Host}
  Write-Host "PROCESS_END=$Label EXIT_CODE=$($proc.ExitCode)"
  if($proc.ExitCode -ne 0){throw "$Label failed with exit code $($proc.ExitCode)"}
}
$cert=$null; $package=$null
try {
  Write-Host 'ROUND_1 — prior run state and immutable evidence boundary'
  $prior=Invoke-RestMethod -Uri 'https://api.github.com/repos/vidift-max/tokensaver-ai/actions/runs/43818990389' -Headers @{ 'User-Agent'='TokenSaver-Phone02-Identity-PoA'; 'Accept'='application/vnd.github+json' }
  Write-Host "PRIOR_RUN=43818990389 STATUS=$($prior.status) CONCLUSION=$($prior.conclusion) SHA=$($prior.head_sha)"
  if($prior.head_sha -ne 'f8fecdf39222d0349afcf008afe6a25df7577d12' -or $prior.conclusion -ne 'cancelled'){throw 'Prior Run evidence state differs from independently audited GitHub API result.'}
  $branch=(git branch --show-current).Trim(); $sha=(git rev-parse HEAD).Trim()
  if($branch -ne 'security/phone-02-windows-identity-poa'){throw "Wrong branch: $branch"}
  Write-Host "BRANCH=$branch"; Write-Host "SHA=$sha"
  Write-Host 'ROUND_2 — source audit and experimental-only scope'
  $source=Join-Path $env:GITHUB_WORKSPACE 'scripts\experimental\phone02-msix-client.cpp'
  if(-not (Test-Path $source)){throw 'Experimental C++ source missing'}
  Write-Host 'AUDIT=GetNamedPipeClientProcessId + impersonated pipe token + process token independently queried by PID.'
  Write-Host 'ROUND_3 — compile packaged client, Supervisor and unpackaged adversary'
  $sdkRoot=Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'; $sdkVersion='10.0.26100.0'
  $makeappx=Join-Path $sdkRoot "$sdkVersion\x64\MakeAppx.exe"; $signtool=Join-Path $sdkRoot "$sdkVersion\x64\SignTool.exe"
  $vswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
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
  Write-Host 'CERT_IMPORT_BEGIN=TrustedPeople'
  Invoke-BoundedTool -FilePath (Join-Path $env:SystemRoot 'System32\certutil.exe') -Arguments ('-user -f -addstore TrustedPeople "'+$certPath+'"') -Label 'certutil-TrustedPeople' -TimeoutSeconds 30
  Write-Host 'CERT_IMPORT_END=TrustedPeople'
  Write-Host 'CERT_IMPORT_BEGIN=LocalMachine-TrustedPeople'
  Invoke-BoundedTool -FilePath (Join-Path $env:SystemRoot 'System32\certutil.exe') -Arguments ('-f -addstore TrustedPeople "'+$certPath+'"') -Label 'certutil-LocalMachine-TrustedPeople' -TimeoutSeconds 30
  $machineTrust=Get-ChildItem Cert:\LocalMachine\TrustedPeople -ErrorAction Stop | Where-Object Thumbprint -eq $cert.Thumbprint
  if(-not $machineTrust){throw 'TEST-ONLY certificate was not found in LocalMachine TrustedPeople after import'}
  Write-Host "MACHINE_TRUST_IMPORT_VERIFIED=TRUE THUMBPRINT=$($machineTrust.Thumbprint)"
  $msix=Join-Path $root 'Phone02IdentityPoA-52881.msix'
  Write-Host 'MAKEAPPX_PACK_BEGIN=TRUE'
  Invoke-BoundedTool -FilePath $makeappx -Arguments ('pack /d "'+$pkg+'" /p "'+$msix+'" /o') -Label 'MakeAppx-pack' -TimeoutSeconds 120
  if(-not (Test-Path $msix)){throw 'MakeAppx returned success but MSIX file is missing'}
  Write-Host "MSIX_CREATED=TRUE SIZE=$((Get-Item $msix).Length)"
  Write-Host 'MAKEAPPX_PACK_END=TRUE'
  Write-Host 'MAKEAPPX_STRUCTURAL_VALIDATION_BEGIN=TRUE'
  $unpack=Join-Path $root 'package-validation'
  New-Item -ItemType Directory -Force -Path $unpack | Out-Null
  Invoke-BoundedTool -FilePath $makeappx -Arguments ('unpack /p "'+$msix+'" /d "'+$unpack+'"') -Label 'MakeAppx-unpack-validation' -TimeoutSeconds 120
  $unpackedManifest=Join-Path $unpack 'AppxManifest.xml'
  $unpackedClient=Join-Path $unpack 'client.exe'
  if(-not (Test-Path $unpackedManifest) -or -not (Test-Path $unpackedClient)){throw 'Unpacked MSIX is missing required manifest or client'}
  $sourceClientHash=(Get-FileHash (Join-Path $pkg 'client.exe') -Algorithm SHA256).Hash
  $unpackedClientHash=(Get-FileHash $unpackedClient -Algorithm SHA256).Hash
  if($sourceClientHash -ne $unpackedClientHash){throw 'Unpacked client hash differs from packaged source'}
  Write-Host "MAKEAPPX_STRUCTURAL_VALIDATION=PASS CLIENT_SHA256=$unpackedClientHash"
  Write-Host 'MAKEAPPX_STRUCTURAL_VALIDATION_END=TRUE'
  Write-Host 'SIGNTOOL_SIGN_BEGIN=TRUE'
  Invoke-BoundedTool -FilePath $signtool -Arguments ('sign /fd SHA256 /sha1 '+$cert.Thumbprint+' "'+$msix+'"') -Label 'SignTool-sign' -TimeoutSeconds 120
  Write-Host 'SIGNTOOL_SIGN_END=TRUE'
  Write-Host 'SIGNTOOL_VERIFY_BEGIN=TRUE'
  Invoke-BoundedTool -FilePath $signtool -Arguments ('verify /pa /v "'+$msix+'"') -Label 'SignTool-verify' -TimeoutSeconds 120
  Write-Host 'SIGNTOOL_VERIFY_END=TRUE'
  Get-FileHash $msix -Algorithm SHA256 | Format-List
  Write-Host 'SIGNATURE_CREATED=TRUE'; Write-Host 'SIGNATURE_VALIDATED=TRUE'
  Write-Host "CERT_SUBJECT=$($cert.Subject)"; Write-Host "CERT_THUMBPRINT=$($cert.Thumbprint)"
  Write-Host 'APPX_INSTALL_BEGIN=TRUE'
  $installCode="Add-AppxPackage -Path '"+$msix.Replace("'","''")+"' -ForceUpdateFromAnyVersion -ErrorAction Stop"
  $installEncoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($installCode))
  Invoke-BoundedTool -FilePath (Join-Path $PSHOME 'pwsh.exe') -Arguments ("-NoLogo -NoProfile -NonInteractive -EncodedCommand "+$installEncoded) -Label 'Add-AppxPackage-install' -TimeoutSeconds 180
  Write-Host 'APPX_INSTALL_COMMAND_END=TRUE'
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
  Write-Host 'APPX_INSTALL_VERIFIED=TRUE'
  $aumid="$($package.PackageFamilyName)!App"
  Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
[ComImport, Guid("2e941141-7f97-4756-ba1d-9decde894a3d"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IApplicationActivationManager {
  [PreserveSig] int ActivateApplication([MarshalAs(UnmanagedType.LPWStr)] string appUserModelId,
    [MarshalAs(UnmanagedType.LPWStr)] string arguments, uint options, out uint processId);
}
[ComImport, Guid("45BA127D-10A8-46EA-8AB7-56EA9078943C"), ClassInterface(ClassInterfaceType.None)]
public class ApplicationActivationManager { }
public static class PackageActivator {
  [DllImport("ole32.dll")] private static extern int CoInitializeEx(IntPtr reserved, uint coInit);
  [DllImport("ole32.dll")] private static extern void CoUninitialize();
  public static uint Activate(string appUserModelId, string arguments) {
    int initHr = CoInitializeEx(IntPtr.Zero, 0x2);
    if (initHr < 0 && initHr != unchecked((int)0x80010106)) Marshal.ThrowExceptionForHR(initHr);
    IApplicationActivationManager manager = null;
    try {
      manager = (IApplicationActivationManager)new ApplicationActivationManager();
      uint pid; int hr = manager.ActivateApplication(appUserModelId, arguments, 0, out pid);
      if (hr < 0) Marshal.ThrowExceptionForHR(hr);
      return pid;
    } finally {
      if (manager != null && Marshal.IsComObject(manager)) Marshal.ReleaseComObject(manager);
      if (initHr >= 0) CoUninitialize();
    }
  }
}
'@
  function Activate-PackagedClient([string]$clientMode='legit') {
    $activationArgs="$($package.PackageFamilyName) $expectedSid $($package.PackageFullName) $clientMode"
    Write-Host "PACKAGE_ACTIVATION_BEGIN AUMID=$aumid ARGS=$activationArgs"
    $activatedProcessId=[PackageActivator]::Activate($aumid,$activationArgs)
    $deadline=(Get-Date).AddSeconds(10)
    do {
      Start-Sleep -Milliseconds 100
      $proc=Get-Process -Id $activatedProcessId -ErrorAction SilentlyContinue
    } while(-not $proc -and (Get-Date) -lt $deadline)
    if(-not $proc){throw "Package activation returned PID $activatedProcessId but process was not observable"}
    $actual=(Get-CimInstance Win32_Process -Filter "ProcessId=$activatedProcessId" -ErrorAction Stop).ExecutablePath
    $expected=Join-Path $package.InstallLocation 'client.exe'
    Write-Host "PACKAGE_ACTIVATION_RESULT=SUCCESS PID=$activatedProcessId PROCESS_PATH=$actual EXPECTED_PATH=$expected"
    if(-not $actual -or $actual -ine $expected){throw "Activated PID $activatedProcessId path mismatch"}
    return [uint32]$activatedProcessId
  }
  function Start-Supervisor([string]$caseName,[string]$customPipeName='',[uint32]$expectedPid=0){
    $out=Join-Path $root "$caseName.supervisor.log";$err=Join-Path $root "$caseName.supervisor.err"
    $expectedPath=Join-Path $package.InstallLocation 'client.exe'
    $pipeName=if($customPipeName){$customPipeName}else{"\\.\pipe\LOCAL\TokenSaver.Phone02.MsixPoA52881.$caseName"}
    $args=@('--supervisor',$package.PackageFamilyName,$expectedSid,$package.PackageFullName,$expectedPath,$caseName,$pipeName,[string]$expectedPid) | ForEach-Object { '"'+$_+'"' }
    $sp=Start-Process -FilePath (Join-Path $root 'supervisor.exe') -ArgumentList ($args -join ' ') -PassThru -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError $err
    $deadline=(Get-Date).AddSeconds(10); $ready=$false
    do {
      Start-Sleep -Milliseconds 100
      if($sp.HasExited){throw "Supervisor exited before pipe readiness: $caseName"}
      $ready=(Get-Content $out -Raw -ErrorAction SilentlyContinue) -match 'SUPERVISOR_READY'
    } while(-not $ready -and (Get-Date) -lt $deadline)
    if(-not $ready){Stop-Process -Id $sp.Id -Force;throw "Supervisor did not publish readiness: $caseName"}
    return @{Process=$sp;Out=$out;Err=$err;PipeName=$pipeName}
  }
  function Finish-Case($s,[string]$caseName){
    if(-not $s.Process.WaitForExit(15000)){Stop-Process -Id $s.Process.Id -Force;throw "Supervisor timeout: $caseName"}
    $content=Get-Content $s.Out -Raw -ErrorAction SilentlyContinue
    Write-Host "CASE_BEGIN=$caseName";Write-Host $content;Write-Host "SUPERVISOR_EXIT=$($s.Process.ExitCode)";Write-Host "CASE_END=$caseName"
    return @{Content=$content;ExitCode=$s.Process.ExitCode}
  }
  Write-Host 'ROUND_6 — legitimate MSIX client and same-user ordinary process'
  $activatedPid=Activate-PackagedClient
  $legitPipe="\\.\pipe\LOCAL\TokenSaver.Phone02.MsixPoA52881.$activatedPid"
  $s=Start-Supervisor 'legitimate-msix' $legitPipe $activatedPid
  $legit=Finish-Case $s 'legitimate-msix'
  $oldProcessDeadline=(Get-Date).AddSeconds(5)
  do { Start-Sleep -Milliseconds 100; $oldProcess=Get-Process -Id $activatedPid -ErrorAction SilentlyContinue } while($oldProcess -and (Get-Date) -lt $oldProcessDeadline)
  Write-Host "INITIAL_CLIENT_PROCESS_EXITED=$(-not [bool]$oldProcess) PID=$activatedPid"
  if($oldProcess){throw 'Initial packaged client process did not exit after pipe completion'}
  if($legit.Content -notmatch "PIPE_CLIENT_PID=$activatedPid(\s|$)"){throw 'Pipe PID does not match IApplicationActivationManager PID'}
  if($legit.ExitCode -ne 0 -or $legit.Content -notmatch 'AUTHORIZATION_RESULT=ALLOW' -or $legit.Content -notmatch 'TOKEN_PROCESS_CORRELATION=PASS' -or $legit.Content -notmatch 'PROCESS_INSTALL_PATH_MATCH=TRUE'){throw 'Legitimate packaged client failed identity/PID/path authorization; stop adversarial interpretation.'}
  Write-Host 'ROUND_7 — protocol spoof matrix'
  $modes=@('common','spoof-package','spoof-package-full','spoof-appcontainer','spoof-package-sid','spoof-handshake','spoof-all')
  foreach($mode in $modes){
    $s=Start-Supervisor $mode
    $cp=Start-Process -FilePath (Join-Path $root 'common.exe') -ArgumentList @('--adversary',$mode,$package.PackageFamilyName,$expectedSid,$package.PackageFullName,$s.PipeName) -PassThru -WindowStyle Hidden
    $result=Finish-Case $s $mode
    if($result.ExitCode -eq 0 -or $result.Content -match 'AUTHORIZATION_RESULT=ALLOW'){throw "FAIL: unauthorized common process allowed in case $mode"}
    if($result.Content -notmatch 'TOKEN_IS_APPCONTAINER=0' -or $result.Content -notmatch 'TOKEN_PROCESS_CORRELATION=PASS'){throw "Common process token/correlation not proven in case $mode"}
    Write-Host "COMMON_PROCESS_REJECTED=$mode"
  }
  $spoofPid=Activate-PackagedClient 'spoof-all'
  $spoofPipe="\\.\pipe\LOCAL\TokenSaver.Phone02.MsixPoA52881.$spoofPid"
  $s=Start-Supervisor 'packaged-client-spoof' $spoofPipe $spoofPid
  $packagedSpoof=Finish-Case $s 'packaged-client-spoof'
  if($packagedSpoof.ExitCode -eq 0 -or $packagedSpoof.Content -match 'AUTHORIZATION_RESULT=ALLOW' -or $packagedSpoof.Content -notmatch 'TOKEN_PROCESS_CORRELATION=PASS'){throw 'Packaged client with false protocol declarations was not safely rejected'}
  Write-Host "PACKAGED_CLIENT_SPOOF_REJECTED=TRUE PID=$spoofPid"
  Write-Host 'ROUND_8 — two package activations and instance identity assessment'
  $pid1=Activate-PackagedClient
  $s1=Start-Supervisor 'instance-1' "\\.\pipe\LOCAL\TokenSaver.Phone02.MsixPoA52881.$pid1" $pid1
  $pid2=Activate-PackagedClient
  $s2=Start-Supervisor 'instance-2' "\\.\pipe\LOCAL\TokenSaver.Phone02.MsixPoA52881.$pid2" $pid2
  Write-Host "INSTANCE_ACTIVATION_PIDS=$pid1,$pid2"
  $r1=Finish-Case $s1 'instance-1'
  $r2=Finish-Case $s2 'instance-2'
  if($r1.ExitCode -ne 0 -or $r2.ExitCode -ne 0){throw 'One of the simultaneous package activation cases failed'}
  if($r1.Content -notmatch "PIPE_CLIENT_PID=$pid1(\s|$)" -or $r2.Content -notmatch "PIPE_CLIENT_PID=$pid2(\s|$)"){throw 'Simultaneous activation PID-to-pipe correlation mismatch'}
  Write-Host "INSTANCE_PIDS=$pid1,$pid2"
  Write-Host "INSTANCE_PIDS_DISTINCT=$($pid1 -ne $pid2)"
  Write-Host 'INSTANCE_MODE=OVERLAPPED_ACTIVATIONS; UNIQUE_INSTANCE_CREDENTIAL=NOT_ESTABLISHED'
  Write-Host 'NO_UNIQUE_INSTANCE_IDENTITY=TRUE — package/AppContainer identity is shared, not a per-process credential.'
  Write-Host 'ROUND_9 — supervisor closure, reconnect and PID/token revalidation'
  $noPipe='\\.\pipe\LOCAL\TokenSaver.Phone02.MsixPoA52881.no-supervisor'
  $noServer=Start-Process -FilePath (Join-Path $root 'common.exe') -ArgumentList @('--adversary','spoof-all',$package.PackageFamilyName,$expectedSid,$package.PackageFullName,$noPipe) -PassThru -Wait
  Write-Host "RECONNECT_WITHOUT_SUPERVISOR_EXIT=$($noServer.ExitCode) (nonzero expected)"; if($noServer.ExitCode -eq 0){throw 'Unexpected successful connection without a Supervisor'}
  $s=Start-Supervisor 'reconnect-common'
  $common=Start-Process -FilePath (Join-Path $root 'common.exe') -ArgumentList @('--adversary','spoof-all',$package.PackageFamilyName,$expectedSid,$package.PackageFullName,$s.PipeName) -PassThru
  $rejected=Finish-Case $s 'reconnect-common'
  if($rejected.ExitCode -eq 0 -or $rejected.Content -match 'AUTHORIZATION_RESULT=ALLOW'){throw 'FAIL: common process authorized after reconnect'}
  $reconnectPid=Activate-PackagedClient
  $reconnectPipe="\\.\pipe\LOCAL\TokenSaver.Phone02.MsixPoA52881.$reconnectPid"
  $s=Start-Supervisor 'reconnect-legitimate' $reconnectPipe $reconnectPid
  $reconnected=Finish-Case $s 'reconnect-legitimate'
  if($reconnected.ExitCode -ne 0 -or $reconnected.Content -notmatch "PIPE_CLIENT_PID=$reconnectPid(\s|$)"){throw 'Legitimate reconnect failed or PID mismatch'}
  if($reconnectPid -eq $activatedPid){throw 'Reconnect reused the original client PID'}
  Write-Host "RECONNECT_NEW_PID=TRUE OLD_PID=$activatedPid NEW_PID=$reconnectPid"
  Write-Host 'ROUND_10 — summary and disposable cleanup'
  Write-Host "LEGITIMATE_ALLOW=$($legit.ExitCode -eq 0)"
  Write-Host "COMMON_REJECT=$($rejected.ExitCode -ne 0)"
  Write-Host "RECONNECT_LEGITIMATE_ALLOW=$($reconnected.ExitCode -eq 0)"
  Write-Host 'POA_RESULT=PARTIAL_PENDING_INDEPENDENT_REVIEW'
}
finally {
  if(-not $package){$package=Get-AppxPackage -Name 'TokenSaver.Phone02.IdentityPoA' -ErrorAction SilentlyContinue}
  if($package){
    Write-Host "CLEANUP_PACKAGE_BEGIN=$($package.PackageFullName)"
    Remove-AppxPackage -Package $package.PackageFullName -ErrorAction SilentlyContinue
    $remaining=Get-AppxPackage -Name 'TokenSaver.Phone02.IdentityPoA' -ErrorAction SilentlyContinue
    Write-Host "CLEANUP_PACKAGE_REMAINS=$([bool]$remaining)"
  }
  if($cert){
    Remove-Item "Cert:\CurrentUser\My\$($cert.Thumbprint)" -ErrorAction SilentlyContinue
    Remove-Item "Cert:\CurrentUser\Root\$($cert.Thumbprint)" -ErrorAction SilentlyContinue
    Remove-Item "Cert:\CurrentUser\TrustedPeople\$($cert.Thumbprint)" -ErrorAction SilentlyContinue
    Remove-Item "Cert:\LocalMachine\TrustedPeople\$($cert.Thumbprint)" -ErrorAction SilentlyContinue
    $myRemaining=Get-ChildItem Cert:\CurrentUser\My -ErrorAction SilentlyContinue | Where-Object Thumbprint -eq $cert.Thumbprint
    $rootRemaining=Get-ChildItem Cert:\CurrentUser\Root -ErrorAction SilentlyContinue | Where-Object Thumbprint -eq $cert.Thumbprint
    $trustedRemaining=Get-ChildItem Cert:\CurrentUser\TrustedPeople -ErrorAction SilentlyContinue | Where-Object Thumbprint -eq $cert.Thumbprint
    $machineRemaining=Get-ChildItem Cert:\LocalMachine\TrustedPeople -ErrorAction SilentlyContinue | Where-Object Thumbprint -eq $cert.Thumbprint
    Write-Host "CLEANUP_CERT_REMAINS=$([bool]($myRemaining -or $rootRemaining -or $trustedRemaining -or $machineRemaining))"
  }
  Stop-Transcript | Out-Null
  Get-ChildItem $root -Force | Select-Object Name,Length | Format-Table
}