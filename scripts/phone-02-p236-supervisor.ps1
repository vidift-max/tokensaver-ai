$ErrorActionPreference='Stop'
$root=Join-Path $env:RUNNER_TEMP 'phone02-p236'
$evidence=Join-Path $env:RUNNER_TEMP 'phone02-p236-evidence'
$build=Join-Path $root 'build';$pkg=Join-Path $root 'package'
New-Item -ItemType Directory -Force -Path $build,$pkg,$evidence|Out-Null
$service='TokenSaverPhone02Supervisor';$created=$false;$installed=$false;$cert=$null;$final='BLOCKED'
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
 $cmd='"'+$vcvars+'" && cl /nologo /EHsc /W4 /std:c++20 /DUNICODE /D_UNICODE /I"'+$build+'" "'+(Join-Path $src 'phone_supervisor.cpp')+'" "'+(Join-Path $build 'phone_supervisor_rpc_s.c')+'" /Fe:"'+$svcExe+'" advapi32.lib rpcrt4.lib bcrypt.lib userenv.lib shell32.lib ole32.lib'
 & cmd.exe /d /s /c $cmd;if($LASTEXITCODE -ne 0){throw 'Supervisor compile failed'}
 $cmd='"'+$vcvars+'" && cl /nologo /EHsc /W4 /std:c++20 /DUNICODE /D_UNICODE /I"'+$build+'" "'+(Join-Path $src 'phone_supervisor_client.cpp')+'" "'+(Join-Path $build 'phone_supervisor_rpc_c.c')+'" /Fe:"'+$cliExe+'" rpcrt4.lib ole32.lib'
 & cmd.exe /d /s /c $cmd;if($LASTEXITCODE -ne 0){throw 'RPC client compile failed'}
 Write-Host ('SUPERVISOR_SHA256='+(Get-FileHash $svcExe -Algorithm SHA256).Hash)
 Write-Host ('CLIENT_SHA256='+(Get-FileHash $cliExe -Algorithm SHA256).Hash)
 $publisher='CN=TokenSaver Phone02 P2.36 TEST ONLY'
 $manifest=@"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10" xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10" IgnorableNamespaces="uap uap10">
<Identity Name="TokenSaver.Phone02.SupervisorTest" Publisher="$publisher" Version="1.0.0.0" ProcessorArchitecture="x64"/>
<Properties><DisplayName>Phone-02 Supervisor P2.36 TEST</DisplayName><PublisherDisplayName>TEST ONLY</PublisherDisplayName><Logo>Assets/StoreLogo.png</Logo></Properties>
<Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0"/></Dependencies>
<Resources><Resource Language="en-us"/></Resources>
<Applications><Application Id="App" Executable="client.exe" uap10:TrustLevel="appContainer" uap10:RuntimeBehavior="packagedClassicApp"><uap:VisualElements DisplayName="Phone-02 Supervisor Test" Description="Disposable P2.36 client" BackgroundColor="transparent" Square150x150Logo="Assets/Square150x150Logo.png" Square44x44Logo="Assets/Square44x44Logo.png"/></Application></Applications>
</Package>
"@
 Set-Content (Join-Path $pkg 'AppxManifest.xml') $manifest -Encoding utf8
 Copy-Item $cliExe (Join-Path $pkg 'client.exe');New-Item -ItemType Directory -Force (Join-Path $pkg 'Assets')|Out-Null
 Add-Type -AssemblyName System.Drawing
 foreach($n in @('StoreLogo.png','Square150x150Logo.png','Square44x44Logo.png')){$bmp=New-Object System.Drawing.Bitmap 64,64;$bmp.Save((Join-Path $pkg ('Assets\'+$n)),[System.Drawing.Imaging.ImageFormat]::Png);$bmp.Dispose()}
 $cert=New-SelfSignedCertificate -Type Custom -Subject $publisher -KeyUsage DigitalSignature -CertStoreLocation 'Cert:\CurrentUser\My' -KeyAlgorithm RSA -KeyLength 2048 -HashAlgorithm SHA256 -NotAfter (Get-Date).AddDays(2)
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
 $binArg='binPath= "'+$svcExe+'" --service "'+$config+'"'
 & sc.exe create $service $binArg 'start= demand' 'obj= NT AUTHORITY\LocalService'|Out-Host
 if($LASTEXITCODE -ne 0){throw 'SCM create failed'};$created=$true
 & sc.exe sidtype $service unrestricted|Out-Host;if($LASTEXITCODE -ne 0){throw 'SCM Service SID setup failed'}
 $sidOut=(& sc.exe showsid $service 2>&1|Out-String);$serviceSid=[regex]::Match($sidOut,'S-1-5-80-(?:\d+-){4}\d+').Value
 if(-not $serviceSid){throw 'Service SID not returned by SCM'}
 @("PACKAGE_FAMILY=$($app.PackageFamilyName)","PACKAGE_FULL=$($app.PackageFullName)","APPCONTAINER_SID=$appSid","SERVICE_SID=$serviceSid")|Set-Content $config -Encoding ASCII
 $serviceAce='NT SERVICE\'+$service+':(OI)(CI)M'
 & icacls.exe $root /inheritance:r /grant 'SYSTEM:(OI)(CI)F' 'BUILTIN\Administrators:(OI)(CI)F' $serviceAce|Out-Host
 if($LASTEXITCODE -ne 0){throw 'Service working directory ACL failed'}
 & sc.exe start $service|Out-Host;if($LASTEXITCODE -ne 0){throw 'SCM service start failed'}
 $s=$null;for($i=0;$i -lt 40;$i++){Start-Sleep -Milliseconds 500;$s=Get-CimInstance Win32_Service -Filter "Name='$service'";if($s.State -eq 'Running'){break}}
 Check ($s.State -eq 'Running') 'SERVICE_START'
 $query=(& sc.exe queryex $service 2>&1|Out-String);$scPid=[regex]::Match($query,'(?im)PID\s*:\s*(\d+)').Groups[1].Value
 $proc=Get-CimInstance Win32_Process -Filter "ProcessId=$($s.ProcessId)"
 Check ($s.StartName -match 'LocalService' -and $s.ProcessId -gt 0 -and $scPid -eq [string]$s.ProcessId -and $proc.ExecutablePath -ieq $svcExe) 'SERVICE_IDENTITY'
 Write-Host ('SERVICE_NAME='+$service);Write-Host ('SERVICE_SID='+$serviceSid);Write-Host ('SERVICE_ACCOUNT='+$s.StartName);Write-Host ('SERVICE_PID='+$s.ProcessId);Write-Host ('SERVICE_SESSION='+$proc.SessionId);Write-Host ('SERVICE_PATH='+$proc.ExecutablePath);Write-Host ('SERVICE_SHA256='+(Get-FileHash $proc.ExecutablePath -Algorithm SHA256).Hash);Write-Host ('SCM_STATE='+$s.State)
 $svcLog=$config+'.log';$deadline=(Get-Date).AddSeconds(20)
 while((-not(Test-Path $svcLog) -or (Get-Content $svcLog -Raw) -notmatch 'RPC_ENDPOINT=READY') -and (Get-Date) -lt $deadline){Start-Sleep -Milliseconds 250}
 $sl=if(Test-Path $svcLog){Get-Content $svcLog -Raw}else{''}
 Check ($sl -match 'SERVICE_SID_IN_TOKEN=TRUE') 'SERVICE_SID_IN_TOKEN'
 Check ($sl -match 'RPC_ENDPOINT=READY') 'RPC_ENDPOINT'
 $clientRoot=Join-Path $env:LOCALAPPDATA ('Packages\'+$app.PackageFamilyName)
 Start-Process explorer.exe -ArgumentList ('shell:AppsFolder\'+$app.PackageFamilyName+'!App')
 $deadline=(Get-Date).AddSeconds(45);$clientLog=$null;$cl=''
 do{Start-Sleep -Milliseconds 250;$clientLog=Get-ChildItem $clientRoot -Filter 'PhoneSupervisor-P236-client.log' -File -Recurse -ErrorAction SilentlyContinue|Select-Object -First 1;if($clientLog){$cl=Get-Content $clientLog.FullName -Raw -ErrorAction SilentlyContinue}}while($cl -notmatch 'CLOSE_SESSION=PASS' -and (Get-Date) -lt $deadline)
 if(-not $clientLog){throw 'Packaged AppContainer client log not found'}
 Copy-Item $clientLog.FullName (Join-Path $evidence 'appcontainer.log') -Force
 Check ($cl -match 'LEGITIMATE_APPCONTAINER=PASS') 'LEGITIMATE_APPCONTAINER'
 Check ($cl -match 'WINDOWS_IDENTITY=PASS') 'WINDOWS_IDENTITY'
 Check ($cl -match 'AUTHORIZATION_ALLOW=PASS') 'AUTHORIZATION_ALLOW'
 Check ($cl -match 'HEARTBEAT=PASS' -and $cl -match 'CLOSE_SESSION=PASS') 'SESSION_LIFECYCLE'
 $common=Join-Path $evidence 'common-process.log';& $cliExe --common *> $common
 Check ($LASTEXITCODE -eq 0 -and (Get-Content $common -Raw) -match 'COMMON_PROCESS_REJECT=PASS') 'COMMON_PROCESS_REJECT'
 $final='PASS'
}
catch {Write-Host ('P236_FAILURE='+$_.Exception.Message);throw}
finally {
 if($created){
  & sc.exe stop $service 2>&1|Out-Host
  $deadline=(Get-Date).AddSeconds(25);$state=''
  do{Start-Sleep -Milliseconds 500;$q=Get-CimInstance Win32_Service -Filter "Name='$service'" -ErrorAction SilentlyContinue;$state=$q.State}while($state -ne 'Stopped' -and (Get-Date) -lt $deadline)
  Write-Host ('SERVICE_SHUTDOWN='+$(if($state -eq 'Stopped'){'PASS'}else{'FAIL'}))
  $svcLog=Join-Path $root 'supervisor.conf.log'
  if(Test-Path $svcLog){$sl=Get-Content $svcLog -Raw;Write-Host ('CLEANUP_COMPLETE='+$(if($sl -match 'SERVICE_CLEANUP_COMPLETE=TRUE'){'PASS'}else{'FAIL'}));Copy-Item $svcLog (Join-Path $evidence 'service.log') -Force}
  if($state -ne 'Stopped'){ $final='BLOCKED'; Write-Host 'SERVICE_SHUTDOWN=FAIL' }
  $svcLog=Join-Path $root 'supervisor.conf.log'
  if(-not(Test-Path $svcLog) -or (Get-Content $svcLog -Raw) -notmatch 'SERVICE_CLEANUP_COMPLETE=TRUE'){ $final='BLOCKED'; Write-Host 'CLEANUP_COMPLETE=FAIL' }
  & sc.exe delete $service|Out-Host
 }
 if($installed){Get-AppxPackage -Name 'TokenSaver.Phone02.SupervisorTest'|Remove-AppxPackage -ErrorAction SilentlyContinue;Write-Host 'MSIX_UNINSTALL=ATTEMPTED'}
 if($cert){Remove-Item ('Cert:\CurrentUser\My\'+$cert.Thumbprint) -Force -ErrorAction SilentlyContinue;Remove-Item ('Cert:\LocalMachine\TrustedPeople\'+$cert.Thumbprint) -Force -ErrorAction SilentlyContinue}
 ('P2.36_FINAL='+$final)|Set-Content (Join-Path $evidence 'classification.txt')
 Stop-Transcript|Out-Null
}
