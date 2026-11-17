$ErrorActionPreference='Stop'
$root=Join-Path $env:RUNNER_TEMP 'phone02-p236'
$evidence=Join-Path $env:RUNNER_TEMP 'phone02-p236-evidence'
$build=Join-Path $root 'build';$pkg=Join-Path $root 'package'
New-Item -ItemType Directory -Force -Path $build,$pkg,$evidence|Out-Null
$service='TokenSaverPhone02Supervisor';$teamRoomBridgePassed=$false;$created=$false;$installed=$false;$cert=$null;$final='BLOCKED';$servicePid=0;$activationPid=0;$testStart=Get-Date;$testPrivateKeyUniqueName=$null;$testPrivateKeyProvider=$null
Start-Transcript -Path (Join-Path $evidence 'transcript.log') -Force|Out-Null
function Check($ok,$label){Write-Host ($label+'='+$(if($ok){'PASS'}else{'FAIL'}));if(-not $ok){throw ($label+' failed')}}
try {
 $x86=[Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
 $vswhere=Join-Path $x86 'Microsoft Visual Studio\Installer\vswhere.exe'
 $vs=(& $vswhere -latest -products '*' -property installationPath).Trim()
 $vcvars=Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'
 $sdk=Get-ChildItem (Join-Path $x86 'Windows Kits\10\bin') -Directory|Where-Object Name -Match '^10\.'|Sort-Object Name -Descending|Select-Object -First 1
 $midl=Join-Path $sdk.FullName 'x64\midl.exe';$makeappx=Join-Path $sdk.FullName 'x64\MakeAppx.exe';$signtool=Join-Path $sdk.FullName 'x64\SignTool.exe'
 foreach($p in @($vcvars,$midl,$makeappx,$signtool)){if(-not(Test-Path $p)){throw ('Missing tool '+$p)}}
 $src=Join-Path $env:GITHUB_WORKSPACE 'services\phone-supervisor'
 $cmdMidl='"'+$vcvars+'" && "'+$midl+'" /nologo /env x64 /h "'+(Join-Path $build 'phone_supervisor_rpc.h')+'" /iid "'+(Join-Path $build 'phone_supervisor_rpc_i.c')+'" /out "'+$build+'" "'+(Join-Path $src 'phone_supervisor_rpc.idl')+'"'
 & cmd.exe /d /s /c $cmdMidl
 if($LASTEXITCODE -ne 0){throw 'MIDL failed'}
 $svcExe=Join-Path $root 'supervisor.exe';$cliExe=Join-Path $root 'client.exe'
 $cmd='"'+$vcvars+'" && cl /nologo /EHsc /W4 /std:c++20 /DUNICODE /D_UNICODE /I"'+$build+'" "'+(Join-Path $src 'phone_supervisor.cpp')+'" "'+(Join-Path $src 'RealSupervisorTeamRoomHttpsAdapter.cpp')+'" "'+(Join-Path $build 'phone_supervisor_rpc_s.c')+'" /Fe:"'+$svcExe+'" advapi32.lib rpcrt4.lib bcrypt.lib userenv.lib shell32.lib ole32.lib winhttp.lib'
 & cmd.exe /d /s /c $cmd;if($LASTEXITCODE -ne 0){throw 'Supervisor compile failed'}
 $cmd='"'+$vcvars+'" && cl /nologo /EHsc /W4 /std:c++20 /DUNICODE /D_UNICODE /I"'+$build+'" "'+(Join-Path $src 'phone_supervisor_client.cpp')+'" "'+(Join-Path $build 'phone_supervisor_rpc_c.c')+'" /Fe:"'+$cliExe+'" rpcrt4.lib ole32.lib'
 & cmd.exe /d /s /c $cmd;if($LASTEXITCODE -ne 0){throw 'RPC client compile failed'}
 Write-Host ('SUPERVISOR_SHA256='+(Get-FileHash $svcExe -Algorithm SHA256).Hash)
 Write-Host ('CLIENT_SHA256='+(Get-FileHash $cliExe -Algorithm SHA256).Hash)
 $publisher='CN=TokenSaver Phone02 P2.36 TEST ONLY'
 $manifest=@"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10" xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10" xmlns:desktop="http://schemas.microsoft.com/appx/manifest/desktop/windows10" xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities" IgnorableNamespaces="uap uap10 desktop rescap">
<Identity Name="TokenSaver.Phone02.SupervisorTest" Publisher="$publisher" Version="1.0.0.0" ProcessorArchitecture="x64"/>
<Properties><DisplayName>Phone-02 Supervisor P2.36 TEST</DisplayName><PublisherDisplayName>TEST ONLY</PublisherDisplayName><Logo>Assets/StoreLogo.png</Logo></Properties>
<Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0"/></Dependencies>
<Resources><Resource Language="en-us"/></Resources>
<Applications><Application Id="App" Executable="client.exe" uap10:TrustLevel="appContainer" uap10:RuntimeBehavior="packagedClassicApp"><uap:VisualElements DisplayName="Phone-02 Supervisor Test" Description="Disposable P2.36 client" BackgroundColor="transparent" Square150x150Logo="Assets/Square150x150Logo.png" Square44x44Logo="Assets/Square44x44Logo.png"/></Application><Application Id="SupervisorHost" Executable="supervisor-host.exe" EntryPoint="Windows.FullTrustApplication"><uap:VisualElements DisplayName="Phone-02 Supervisor Host TEST" Description="Disposable full-trust activation host" BackgroundColor="transparent" Square150x150Logo="Assets/Square150x150Logo.png" Square44x44Logo="Assets/Square44x44Logo.png"/><Extensions><desktop:Extension Category="windows.fullTrustProcess" Executable="supervisor.exe"><desktop:FullTrustProcess/></desktop:Extension></Extensions></Application></Applications>
<Capabilities><rescap:Capability Name="runFullTrust"/></Capabilities>
</Package>
"@
 Set-Content (Join-Path $pkg 'AppxManifest.xml') $manifest -Encoding utf8
 Copy-Item $cliExe (Join-Path $pkg 'client.exe');Copy-Item $cliExe (Join-Path $pkg 'supervisor-host.exe');Copy-Item $svcExe (Join-Path $pkg 'supervisor.exe');New-Item -ItemType Directory -Force (Join-Path $pkg 'Assets')|Out-Null
 Add-Type -AssemblyName System.Drawing
 foreach($n in @('StoreLogo.png','Square150x150Logo.png','Square44x44Logo.png')){$bmp=New-Object System.Drawing.Bitmap 64,64;$bmp.Save((Join-Path $pkg ('Assets\'+$n)),[System.Drawing.Imaging.ImageFormat]::Png);$bmp.Dispose()}
 $cert=New-SelfSignedCertificate -Type Custom -Subject $publisher -KeyUsage DigitalSignature -CertStoreLocation 'Cert:\CurrentUser\My' -KeyAlgorithm RSA -KeyLength 2048 -HashAlgorithm SHA256 -NotAfter (Get-Date).AddDays(2)
 $rsa=[System.Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey($cert)
 if($rsa -is [System.Security.Cryptography.RSACng]){$testPrivateKeyUniqueName=$rsa.Key.UniqueName;$testPrivateKeyProvider=$rsa.Key.Provider}
 if($rsa){$rsa.Dispose()}
 if(-not $testPrivateKeyUniqueName -or -not $testPrivateKeyProvider){throw 'Could not identify disposable MSIX signing private key for cleanup verification'}
 $cer=Join-Path $root 'test.cer';Export-Certificate -Cert $cert -FilePath $cer|Out-Null
 & certutil.exe -f -addstore TrustedPeople $cer|Out-Host;if($LASTEXITCODE -ne 0){throw 'Test certificate trust failed'}
 $msix=Join-Path $root 'phone02-p236.msix'
 & $makeappx pack /d $pkg /p $msix /o|Out-Host;if($LASTEXITCODE -ne 0){throw 'MakeAppx failed'}
 & $signtool sign /fd SHA256 /sha1 $cert.Thumbprint $msix|Out-Host;if($LASTEXITCODE -ne 0){throw 'MSIX signing failed'}
 & $signtool verify /pa /v $msix|Out-Host;if($LASTEXITCODE -ne 0){throw 'MSIX signature verify failed'}
 Write-Host ('MSIX_SHA256='+(Get-FileHash $msix -Algorithm SHA256).Hash)
 Add-AppxPackage -Path $msix -ErrorAction Stop;$installed=$true
 $app=Get-AppxPackage -Name 'TokenSaver.Phone02.SupervisorTest'|Select-Object -First 1
 if(-not $app){throw 'MSIX package registration missing'}
 if($app.Publisher -ne $publisher){throw 'Installed package publisher does not match the disposable test signer'}
 Write-Host ('PACKAGE_PUBLISHER='+$app.Publisher)
 Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public static class P236Sid{[DllImport("userenv.dll",CharSet=CharSet.Unicode)]public static extern int DeriveAppContainerSidFromAppContainerName(string n,out IntPtr s);[DllImport("advapi32.dll",CharSet=CharSet.Unicode)]public static extern bool ConvertSidToStringSid(IntPtr s,out IntPtr t);[DllImport("kernel32.dll")]public static extern IntPtr LocalFree(IntPtr h);}'
 $sp=[IntPtr]::Zero;$hr=[P236Sid]::DeriveAppContainerSidFromAppContainerName($app.PackageFamilyName,[ref]$sp);if($hr -ne 0){throw 'AppContainer SID derive failed'}
 $tp=[IntPtr]::Zero;if(-not [P236Sid]::ConvertSidToStringSid($sp,[ref]$tp)){throw 'AppContainer SID stringify failed'}
 $appSid=[Runtime.InteropServices.Marshal]::PtrToStringUni($tp);[P236Sid]::LocalFree($tp)|Out-Null;[P236Sid]::LocalFree($sp)|Out-Null
 $config=Join-Path $root 'supervisor.conf';$svcExe=Join-Path $root 'supervisor.exe'
 $binPathValue='"'+$svcExe+'" --service "'+$config+'"'
 & sc.exe create $service 'binPath=' $binPathValue 'start=' 'demand' 'obj=' 'NT AUTHORITY\LocalService'|Out-Host
 if($LASTEXITCODE -ne 0){throw 'SCM create failed'};$created=$true
 & sc.exe sidtype $service unrestricted|Out-Host;if($LASTEXITCODE -ne 0){throw 'SCM Service SID setup failed'}
 $sidOut=(& sc.exe showsid $service 2>&1|Out-String);$serviceSid=[regex]::Match($sidOut,'S-1-5-80-(?:\d+-){4}\d+').Value
 if(-not $serviceSid){throw 'Service SID not returned by SCM'}
 @("PACKAGE_FAMILY=$($app.PackageFamilyName)","PACKAGE_FULL=$($app.PackageFullName)","APPCONTAINER_SID=$appSid","RPC_CLIENT_APPCONTAINER_SID=$appSid","SERVICE_SID=$serviceSid","USER_SID=$([System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value)","RPC_CLIENT_USER_SID=$([System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value)","TEAM_ROOM_FAKE_MODE=1","TEAM_ROOM_TEST_AI_PHONE_ID=TEST-AI-PHONE-02","TEAM_ROOM_TEST_ENDPOINT_ID=TEST-ENDPOINT-02","TEAM_ROOM_TEST_RECIPIENT_ID=FAKE-RECIPIENT-PHONE")|Set-Content $config -Encoding ASCII
 $serviceAce='NT SERVICE\'+$service+':(OI)(CI)M'
 & icacls.exe $root /inheritance:r /grant 'SYSTEM:(OI)(CI)F' 'BUILTIN\Administrators:(OI)(CI)F' $serviceAce|Out-Host
 if($LASTEXITCODE -ne 0){throw 'Service working directory ACL failed'}
 & sc.exe start $service|Out-Host;if($LASTEXITCODE -ne 0){throw 'SCM service start failed'}
 $s=$null;for($i=0;$i -lt 40;$i++){Start-Sleep -Milliseconds 500;$s=Get-CimInstance Win32_Service -Filter "Name='$service'";if($s.State -eq 'Running'){break}}
 Check ($s.State -eq 'Running') 'SERVICE_START'
 $query=(& sc.exe queryex $service 2>&1|Out-String);$scPid=[regex]::Match($query,'(?im)PID\s*:\s*(\d+)').Groups[1].Value
 $proc=Get-CimInstance Win32_Process -Filter "ProcessId=$($s.ProcessId)"
 Check ($s.StartName -match 'LocalService' -and $s.ProcessId -gt 0 -and $scPid -eq [string]$s.ProcessId -and $proc.ExecutablePath -ieq $svcExe) 'SERVICE_IDENTITY'
 $servicePid=[int]$s.ProcessId
 Write-Host ('SERVICE_NAME='+$service);Write-Host ('SERVICE_SID='+$serviceSid);Write-Host ('SERVICE_ACCOUNT='+$s.StartName);Write-Host ('SERVICE_PID='+$s.ProcessId);Write-Host ('SERVICE_SESSION='+$proc.SessionId);Write-Host ('SERVICE_PATH='+$proc.ExecutablePath);Write-Host ('SERVICE_SHA256='+(Get-FileHash $proc.ExecutablePath -Algorithm SHA256).Hash);Write-Host ('SCM_STATE='+$s.State)
 $svcLog=$config+'.log';$deadline=(Get-Date).AddSeconds(20)
 while((-not(Test-Path $svcLog) -or (Get-Content $svcLog -Raw) -notmatch 'RPC_ENDPOINT=READY') -and (Get-Date) -lt $deadline){Start-Sleep -Milliseconds 250}
 $sl=if(Test-Path $svcLog){Get-Content $svcLog -Raw}else{''}
 Check ($sl -match 'SERVICE_SID_IN_TOKEN=TRUE') 'SERVICE_SID_IN_TOKEN'
 Check ($sl -match 'RPC_ENDPOINT=READY') 'RPC_ENDPOINT'
 Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
[ComImport,Guid("2e941141-7f97-4756-ba1d-9decde894a3d"),InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IP236ApplicationActivationManager { [PreserveSig]int ActivateApplication([MarshalAs(UnmanagedType.LPWStr)]string appUserModelId,[MarshalAs(UnmanagedType.LPWStr)]string arguments,uint options,out uint processId); }
[ComImport,Guid("45BA127D-10A8-46EA-8AB7-56EA9078943C"),ClassInterface(ClassInterfaceType.None)]
public class P236ApplicationActivationManager {}
public static class P236Activator {
 [DllImport("ole32.dll")] private static extern int CoInitializeEx(IntPtr p,uint flags);
 [DllImport("ole32.dll")] private static extern void CoUninitialize();
 public static uint Activate(string id,string args) {
  int init=CoInitializeEx(IntPtr.Zero,2);
  if(init<0 && init!=unchecked((int)0x80010106)) Marshal.ThrowExceptionForHR(init);
  IP236ApplicationActivationManager m=null; bool initialized=init>=0;
  try { m=(IP236ApplicationActivationManager)new P236ApplicationActivationManager(); uint pid;
   int hr=m.ActivateApplication(id,args,0,out pid); if(hr<0)Marshal.ThrowExceptionForHR(hr); return pid;
  } finally { if(m!=null && Marshal.IsComObject(m))Marshal.ReleaseComObject(m); if(initialized)CoUninitialize(); }
 }
}
'@
 $aumid="$($app.PackageFamilyName)!App"
 $activationPid=[P236Activator]::Activate($aumid,"")
 Write-Host "ACTIVATED_APP_PID=$activationPid AUMID=$aumid"
 $clientRoot=Join-Path $env:LOCALAPPDATA ('Packages\'+$app.PackageFamilyName)
 $fallbackLog=Join-Path $env:LOCALAPPDATA 'PhoneSupervisor-P236-client.log'
 $deadline=(Get-Date).AddSeconds(45);$clientLog=$null;$cl=''
 do{
  Start-Sleep -Milliseconds 250
  if(Test-Path $fallbackLog){$clientLog=Get-Item $fallbackLog}
  if(-not $clientLog){$clientLog=Get-ChildItem $clientRoot -Filter 'PhoneSupervisor-P236-client.log' -File -Recurse -ErrorAction SilentlyContinue|Select-Object -First 1}
  if($clientLog){$cl=Get-Content $clientLog.FullName -Raw -ErrorAction SilentlyContinue}
 }while($cl -notmatch 'CLOSE_SESSION=PASS' -and (Get-Date) -lt $deadline)
 if(-not $clientLog){
  $activated=Get-CimInstance Win32_Process -Filter "ProcessId=$activationPid" -ErrorAction SilentlyContinue
  Write-Host "ACTIVATED_PROCESS_STILL_PRESENT=$([bool]$activated) IMAGE=$($activated.ExecutablePath)"
  throw 'Packaged AppContainer client log not found'
 }
 Copy-Item $clientLog.FullName (Join-Path $evidence 'appcontainer.log') -Force
 Check ($cl -match 'LEGITIMATE_APPCONTAINER=PASS') 'LEGITIMATE_APPCONTAINER'
 Check ($cl -match 'WINDOWS_IDENTITY=PASS') 'WINDOWS_IDENTITY'
 Check ($cl -match 'AUTHORIZATION_ALLOW=PASS') 'AUTHORIZATION_ALLOW'
 Check ($cl -match 'HEARTBEAT=PASS' -and $cl -match 'TEAM_ROOM_SCENARIO=PASS' -and $cl -match 'CLOSE_SESSION=PASS') 'SESSION_AND_TEAM_ROOM_LIFECYCLE'
 $teamRoomLog=Get-Content $svcLog -Raw
 foreach($marker in @('TEAM_ROOM_AUTH_FAILURE=PASS','TEAM_ROOM_AUTHENTICATE=PASS','TEAM_ROOM_IDENTITY_MISMATCH=PASS','TEAM_ROOM_SESSION_EXECUTION_NAMESPACE=PASS','TEAM_ROOM_ATTACH=PASS','TEAM_ROOM_HEARTBEAT=PASS','TEAM_ROOM_RECONNECT=PASS','TEAM_ROOM_PAIR_DENY=PASS','TEAM_ROOM_SEND=PASS','TEAM_ROOM_SENDER_IDENTITY=PASS','TEAM_ROOM_RECEIVE=PASS','TEAM_ROOM_ACK=PASS','TEAM_ROOM_CREDENTIALS_SUPERVISOR_ONLY=PASS','TEAM_ROOM_DETACH=PASS','TEAM_ROOM_REVOKE=PASS','TEAM_ROOM_FAKE_SCENARIO=PASS')){Check ($teamRoomLog -match [regex]::Escape($marker)) $marker}
 $teamRoomBridgePassed=$true
 $common=Join-Path $evidence 'common-process.log';& $cliExe --common *> $common
 $commonText=Get-Content $common -Raw
 Check ($LASTEXITCODE -eq 0 -and $commonText -match 'COMMON_PROCESS_REJECT=PASS' -and $commonText -match 'COMMON_PROCESS_TEAM_ROOM_REJECT=PASS') 'COMMON_PROCESS_REJECT'
 $svcLog=Join-Path $root 'supervisor.conf.log';$commonServiceLog=Get-Content $svcLog -Raw
 Check ($commonServiceLog -match 'token_is_appcontainer=0' -and $commonServiceLog -match 'reason=TOKEN_IS_NOT_APPCONTAINER' -and $commonServiceLog -match 'user_sid=S-1-5-21-') 'COMMON_PROCESS_WINDOWS_IDENTITY'
 Copy-Item $svcLog (Join-Path $evidence 'service-allow-and-common-reject.log') -Force
 $idlText=Get-Content (Join-Path $env:GITHUB_WORKSPACE 'services\phone-supervisor\phone_supervisor_rpc.idl') -Raw
 Check ($idlText -notmatch '(?im)^\s*\[(?:in|in,out)[^\]]*\]\s*(?:unsigned\s+)?(?:long|wchar_t\s*\*)\s*(?:pid|user_sid|appcontainer_sid|package_family|package_full|ai_phone_id|ai_id)\b') 'CLIENT_IDENTITY_CLAIMS_ABSENT'
 $idlExecutable=($idlText -split "`r?`n" | Where-Object { $_ -notmatch '^\s*//' }) -join "`n"
 Check ($idlExecutable -notmatch '(?i)\b(ai_phone_id|sender_ai_phone_id|principal_id|execution_id|token|credential|secret)\b') 'TEAM_ROOM_RPC_NO_SECRETS_OR_AI_IDENTITY'
 $expectedUser=[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
 $mismatchCases=@(
  @{Name='USER_SID_MISMATCH';UserSid='S-1-5-21-1-2-3-999';AppSid=$appSid;Reason='USER_SID_PIN_MISMATCH'},
  @{Name='APPCONTAINER_SID_MISMATCH';UserSid=$expectedUser;AppSid='S-1-15-2-1-2-3-4-5-6-7';Reason='APPCONTAINER_SID_PIN_MISMATCH'}
 )
 foreach($tc in $mismatchCases){
  & sc.exe stop $service 2>&1|Out-Host
  $deadline=(Get-Date).AddSeconds(15);$state=''
  do{Start-Sleep -Milliseconds 250;$q=Get-CimInstance Win32_Service -Filter "Name='$service'" -ErrorAction SilentlyContinue;$state=$q.State}while($state -ne 'Stopped' -and (Get-Date) -lt $deadline)
  Check ($state -eq 'Stopped' -and (Get-Content $svcLog -Raw) -match 'SERVICE_CLEANUP_COMPLETE=TRUE SESSIONS=0') ($tc.Name+'_PREVIOUS_SESSION_CLEAN')
  Copy-Item $svcLog (Join-Path $evidence ('service-before-'+$tc.Name+'.log')) -Force
  @("PACKAGE_FAMILY=$($app.PackageFamilyName)","PACKAGE_FULL=$($app.PackageFullName)","APPCONTAINER_SID=$($tc.AppSid)","RPC_CLIENT_APPCONTAINER_SID=$appSid","SERVICE_SID=$serviceSid","USER_SID=$($tc.UserSid)","RPC_CLIENT_USER_SID=$expectedUser")|Set-Content $config -Encoding ASCII
  Remove-Item $svcLog -Force -ErrorAction SilentlyContinue
  & sc.exe start $service|Out-Host;if($LASTEXITCODE -ne 0){throw ('SCM restart failed for '+$tc.Name)}
  $s=$null;for($i=0;$i -lt 40;$i++){Start-Sleep -Milliseconds 250;$s=Get-CimInstance Win32_Service -Filter "Name='$service'";if($s.State -eq 'Running'){break}}
  Check ($s.State -eq 'Running') ($tc.Name+'_SERVICE_START')
  $servicePid=[int]$s.ProcessId
  $deadline=(Get-Date).AddSeconds(15)
  while((-not(Test-Path $svcLog) -or (Get-Content $svcLog -Raw) -notmatch 'RPC_ENDPOINT=READY') -and (Get-Date) -lt $deadline){Start-Sleep -Milliseconds 250}
  Check ((Test-Path $svcLog) -and (Get-Content $svcLog -Raw) -match 'RPC_ENDPOINT=READY') ($tc.Name+'_RPC_ENDPOINT')
  Remove-Item $fallbackLog -Force -ErrorAction SilentlyContinue
  if(Test-Path $clientRoot){Get-ChildItem $clientRoot -Filter 'PhoneSupervisor-P236-client.log' -File -Recurse -ErrorAction SilentlyContinue|Remove-Item -Force -ErrorAction SilentlyContinue}
  $activationPid=[P236Activator]::Activate($aumid,"")
  $deadline=(Get-Date).AddSeconds(20);$rejectLog=$null;$rejectText=''
  do{Start-Sleep -Milliseconds 250
   if(Test-Path $fallbackLog){$rejectLog=Get-Item $fallbackLog}
   if(-not $rejectLog){$rejectLog=Get-ChildItem $clientRoot -Filter 'PhoneSupervisor-P236-client.log' -File -Recurse -ErrorAction SilentlyContinue|Select-Object -First 1}
   if($rejectLog){$rejectText=Get-Content $rejectLog.FullName -Raw -ErrorAction SilentlyContinue}
  }while($rejectText -notmatch 'LEGITIMATE_APPCONTAINER=FAIL' -and (Get-Date) -lt $deadline)
  Check ($rejectText -match 'LEGITIMATE_APPCONTAINER=FAIL' -and $rejectText -notmatch 'AUTHORIZATION_ALLOW=PASS') ($tc.Name+'_CLIENT_DENIED')
  $mismatchLog=Get-Content $svcLog -Raw
  Check ($mismatchLog -match $tc.Reason -and $mismatchLog -match 'RPC_BEGIN .*result=REJECT') ($tc.Name+'_WINDOWS_IDENTITY_REJECT')
  Copy-Item $rejectLog.FullName (Join-Path $evidence ($tc.Name+'-client.log')) -Force
  & sc.exe stop $service|Out-Host
  $deadline=(Get-Date).AddSeconds(15);$state=''
  do{Start-Sleep -Milliseconds 250;$q=Get-CimInstance Win32_Service -Filter "Name='$service'" -ErrorAction SilentlyContinue;$state=$q.State}while($state -ne 'Stopped' -and (Get-Date) -lt $deadline)
  $mismatchLog=Get-Content $svcLog -Raw
  Check ($state -eq 'Stopped' -and $mismatchLog -match 'SERVICE_CLEANUP_COMPLETE=TRUE SESSIONS=0') ($tc.Name+'_SERVICE_CLEANUP')
  Copy-Item $svcLog (Join-Path $evidence ($tc.Name+'-service.log')) -Force
 }
 $final='PASS'
}
catch {Write-Host ('P236_FAILURE='+$_.Exception.Message);throw}
finally {
 $serviceGraceful=$false;$serviceGone=(-not $created);$clientGone=$false;$packageGone=(-not $installed);$certificateGone=$true;$privateKeyGone=$false;$tempGone=$false
 if($created){
  & sc.exe stop $service 2>&1|Out-Host
  $deadline=(Get-Date).AddSeconds(20);$state=''
  do{Start-Sleep -Milliseconds 500;$q=Get-CimInstance Win32_Service -Filter "Name='$service'" -ErrorAction SilentlyContinue;$state=$q.State}while($state -ne 'Stopped' -and (Get-Date) -lt $deadline)
  $svcLog=Join-Path $root 'supervisor.conf.log'
  if(Test-Path $svcLog){
   $sl=Get-Content $svcLog -Raw
   Copy-Item $svcLog (Join-Path $evidence 'service.log') -Force
   $serviceGraceful=($state -eq 'Stopped' -and $sl -match 'SERVICE_CLEANUP_COMPLETE=TRUE')
  }
  if(-not $serviceGraceful -and $servicePid -gt 0){
   $sp=Get-CimInstance Win32_Process -Filter "ProcessId=$servicePid" -ErrorAction SilentlyContinue
   if($sp){Stop-Process -Id $servicePid -Force -ErrorAction SilentlyContinue}
  }
  $deadline=(Get-Date).AddSeconds(10)
  do{Start-Sleep -Milliseconds 250;$sp=if($servicePid -gt 0){Get-CimInstance Win32_Process -Filter "ProcessId=$servicePid" -ErrorAction SilentlyContinue}else{$null}}while($sp -and (Get-Date) -lt $deadline)
  Write-Host ('SERVICE_SHUTDOWN='+$(if($serviceGraceful){'PASS'}else{'FAIL'}))
  & sc.exe delete $service|Out-Host
  $deadline=(Get-Date).AddSeconds(10);$q=$null
  do{Start-Sleep -Milliseconds 250;$q=Get-CimInstance Win32_Service -Filter "Name='$service'" -ErrorAction SilentlyContinue}while($q -and (Get-Date) -lt $deadline)
  $serviceGone=($null -eq $q)
 }
 if($activationPid -gt 0){
  $ap=Get-CimInstance Win32_Process -Filter "ProcessId=$activationPid" -ErrorAction SilentlyContinue
  if($ap){Stop-Process -Id $activationPid -Force -ErrorAction SilentlyContinue}
 }
 $leftover=@(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue|Where-Object { $_.ExecutablePath -and ($_.ExecutablePath -ieq (Join-Path $root 'client.exe') -or $_.ExecutablePath -ieq (Join-Path $root 'supervisor.exe') -or $_.ExecutablePath -ieq (Join-Path $pkg 'client.exe')) })
 foreach($p in $leftover){Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue}
 Start-Sleep -Milliseconds 500
 $leftover=@(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue|Where-Object { $_.ExecutablePath -and ($_.ExecutablePath -ieq (Join-Path $root 'client.exe') -or $_.ExecutablePath -ieq (Join-Path $root 'supervisor.exe') -or $_.ExecutablePath -ieq (Join-Path $pkg 'client.exe')) })
 $clientGone=($leftover.Count -eq 0)
 if($installed){
  Get-AppxPackage -Name 'TokenSaver.Phone02.SupervisorTest'|Remove-AppxPackage -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 1
  try{$packageGone=(@(Get-AppxPackage -Name 'TokenSaver.Phone02.SupervisorTest' -ErrorAction Stop).Count -eq 0)}catch{$packageGone=$false}
  Write-Host ('MSIX_UNINSTALL='+$(if($packageGone){'PASS'}else{'FAIL'}))
 }
 if($cert){
  foreach($store in @('Cert:\CurrentUser\My','Cert:\CurrentUser\TrustedPeople','Cert:\LocalMachine\My','Cert:\LocalMachine\TrustedPeople')){
   Remove-Item ($store+'\'+$cert.Thumbprint) -Force -ErrorAction SilentlyContinue
  }
  foreach($store in @('Cert:\CurrentUser\My','Cert:\CurrentUser\TrustedPeople','Cert:\LocalMachine\My','Cert:\LocalMachine\TrustedPeople')){
   if(Get-ChildItem $store -ErrorAction SilentlyContinue|Where-Object Thumbprint -EQ $cert.Thumbprint){$certificateGone=$false}
  }
 }
 if($testPrivateKeyUniqueName -and $testPrivateKeyProvider){
  try{$k=[System.Security.Cryptography.CngKey]::Open($testPrivateKeyUniqueName,$testPrivateKeyProvider,[System.Security.Cryptography.CngKeyOpenOptions]::UserKey);$k.Delete();$k.Dispose()}catch{}
  try{$k2=[System.Security.Cryptography.CngKey]::Open($testPrivateKeyUniqueName,$testPrivateKeyProvider,[System.Security.Cryptography.CngKeyOpenOptions]::UserKey);$k2.Dispose();$privateKeyGone=$false}catch{$privateKeyGone=$true}
 }
 if(-not $serviceGraceful){
  $eventPath=Join-Path $evidence 'windows-events.txt'
  foreach($ln in @('Application','System')){
   try{Get-WinEvent -FilterHashtable @{LogName=$ln;StartTime=$testStart;ProviderName=@('Application Error','Service Control Manager','Windows Error Reporting')} -MaxEvents 30 -ErrorAction SilentlyContinue|Select-Object TimeCreated,Id,ProviderName,LevelDisplayName,Message|Format-List|Out-String|Add-Content $eventPath}catch{}
  }
 }
 if(Test-Path $root){Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue}
 $tempGone=(-not(Test-Path $root))
 Write-Host ('SERVICE_REMOVED='+$(if($serviceGone){'PASS'}else{'FAIL'}))
 Write-Host ('CLIENT_PROCESSES_REMOVED='+$(if($clientGone){'PASS'}else{'FAIL'}))
 Write-Host ('MSIX_REMOVED='+$(if($packageGone){'PASS'}else{'FAIL'}))
 Write-Host ('TEST_CERT_REMOVED='+$(if($certificateGone){'PASS'}else{'FAIL'}))
 Write-Host ('TEST_PRIVATE_KEY_REMOVED='+$(if($privateKeyGone){'PASS'}else{'FAIL'}))
 Write-Host ('TEMP_DIRECTORY_REMOVED='+$(if($tempGone){'PASS'}else{'FAIL'}))
 $logText='';if(Test-Path (Join-Path $evidence 'service.log')){$logText=Get-Content (Join-Path $evidence 'service.log') -Raw}
 $sessionClean=($logText -match 'SERVICE_CLEANUP_COMPLETE=TRUE SESSIONS=0')
 $cleanup=($serviceGone -and $clientGone -and $packageGone -and $certificateGone -and $privateKeyGone -and $tempGone -and $sessionClean)
 Write-Host ('CLEANUP_COMPLETE='+$(if($cleanup){'PASS'}else{'FAIL'}))
 if($teamRoomBridgePassed -and $cleanup -and $final -eq 'PASS'){'P2.37_WINDOWS_FAKE_BRIDGE=PASS'|Set-Content (Join-Path $evidence 'p2.37-classification.txt') -Encoding ASCII}else{'P2.37_WINDOWS_FAKE_BRIDGE=BLOCKED'|Set-Content (Join-Path $evidence 'p2.37-classification.txt') -Encoding ASCII}
 if(-not $cleanup){$final='BLOCKED'}
 ('P2.36.1_FINAL='+$final)|Set-Content (Join-Path $evidence 'classification.txt')
 Stop-Transcript|Out-Null
}
