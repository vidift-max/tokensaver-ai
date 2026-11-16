$ErrorActionPreference='Stop'
$baseline='3b614efc7ce4b1a83e05c94d55c42c0cf9c04cb1'
$repo='vidift-max/tokensaver-ai';$branch='security/phone-02-windows-identity-poa'
$root=Join-Path $env:RUNNER_TEMP 'phone02-p234-rpc-evidence';$pkg=Join-Path $root 'package';$rpcDir=Join-Path $root 'rpc'
New-Item -ItemType Directory -Force -Path $pkg,$rpcDir|Out-Null
Start-Transcript -Path (Join-Path $root 'p234-transcript.log') -Force|Out-Null
$cert=$null;$package=$null;$finalClassification='BLOCKED'
$rpcServers=[System.Collections.Generic.List[System.Diagnostics.Process]]::new()
$activatedClientPids=[System.Collections.Generic.List[uint32]]::new()
$flags=[ordered]@{Legitimate=$false;CommonReject=$false;SpoofPid=$false;SpoofApp=$false;SpoofSid=$false;SpoofFamily=$false;SpoofFull=$false;SpoofPackage=$false;TwoInstances=$false;Reconnect=$false;Replay=$false;Cleanup=$false;SecurityAudit=$false;EvidenceAudit=$false;ProductionUntouched=$false}
function Round([int]$n,[string]$d){Write-Host ("ROUND_{0:D2} — {1}" -f $n,$d)}
function RunTool([string]$exe,[string]$arguments,[string]$label,[int]$seconds=120){
 $o=Join-Path $root "$label.out.log";$e=Join-Path $root "$label.err.log"
 Write-Host "TOOL_EXECUTABLE=$exe TOOL_ARGUMENTS=$arguments LABEL=$label"
 $p=Start-Process -FilePath $exe -ArgumentList $arguments -PassThru -NoNewWindow -RedirectStandardOutput $o -RedirectStandardError $e
 if(-not $p.WaitForExit($seconds*1000)){try{$p.Kill()}catch{};throw "$label timeout"}
 if(Test-Path $o){Get-Content $o -Raw|Write-Host};if(Test-Path $e){Get-Content $e -Raw|Write-Host}
 if($p.ExitCode -ne 0){throw "$label failed exit=$($p.ExitCode)"}
}
function Get-ClientLog([uint32]$clientPid){
 $name="Phone02-MsixPoA-rpc-client-$clientPid.log";$base=Join-Path $env:LOCALAPPDATA "Packages\$($script:package.PackageFamilyName)"
 $m=$null;if(Test-Path $base){$m=Get-ChildItem $base -Filter $name -File -Recurse -ErrorAction SilentlyContinue|Select-Object -First 1}
 if(-not $m){$m=Get-ChildItem $env:LOCALAPPDATA -Filter $name -File -Recurse -ErrorAction SilentlyContinue|Select-Object -First 1}
 return $m
}
function Wait-ClientLog([uint32]$clientPid,[string]$marker,[int]$seconds=25){
 $deadline=(Get-Date).AddSeconds($seconds);$m=$null;$content=''
 do{Start-Sleep -Milliseconds 150;$m=Get-ClientLog $clientPid;if($m){$content=Get-Content $m.FullName -Raw -ErrorAction SilentlyContinue}}while(($content -notmatch [regex]::Escape($marker)) -and (Get-Date) -lt $deadline)
 if(-not $m -or $content -notmatch [regex]::Escape($marker)){throw "Client log marker missing: pid=$clientPid marker=$marker content=$content"}
 Copy-Item $m.FullName (Join-Path $root $m.Name) -Force -ErrorAction SilentlyContinue
 return @{Path=$m.FullName;Content=$content}
}
function Start-RpcServer([string]$case){
 $endpoint='P234_'+[guid]::NewGuid().ToString('N').Substring(0,24);$out=Join-Path $root "$case.server.log";$err=Join-Path $root "$case.server.err"
 $clientPath=Join-Path $script:package.InstallLocation 'client.exe'
 $a=@('--rpc-supervisor',$script:package.PackageFamilyName,$script:expectedSid,$script:package.PackageFullName,$clientPath,$endpoint)|ForEach-Object {'"'+($_ -replace '"','\"')+'"'}
 $p=Start-Process (Join-Path $root 'supervisor.exe') -ArgumentList ($a -join ' ') -PassThru -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError $err
 $script:rpcServers.Add($p);$deadline=(Get-Date).AddSeconds(15);$content=''
 do{Start-Sleep -Milliseconds 100;if($p.HasExited){$content=Get-Content $out -Raw -ErrorAction SilentlyContinue;throw "RPC server exited $case code=$($p.ExitCode) log=$content"};$content=Get-Content $out -Raw -ErrorAction SilentlyContinue}while($content -notmatch 'RPC_SERVER_READY=TRUE' -and (Get-Date) -lt $deadline)
 if($content -notmatch 'RPC_SERVER_READY=TRUE'){Stop-Process $p.Id -Force;throw "RPC server not ready: $case $content"}
 Write-Host "RPC_SERVER_READY=TRUE CASE=$case PID=$($p.Id) ENDPOINT=$endpoint"
 return @{Process=$p;Out=$out;Endpoint=$endpoint;Case=$case}
}
function Finish-RpcServer($s){
 if(-not $s.Process.WaitForExit(20000)){try{$s.Process.Kill()}catch{};throw "RPC server timeout: $($s.Case)"}
 $content=Get-Content $s.Out -Raw -ErrorAction SilentlyContinue;Write-Host "RPC_SERVER_EXIT CASE=$($s.Case) CODE=$($s.Process.ExitCode)";Write-Host $content
 return @{Content=$content;Exit=$s.Process.ExitCode}
}
function Activate-Client([string]$mode,[string]$endpoint){
 $arguments="$($script:package.PackageFamilyName) $script:expectedSid $($script:package.PackageFullName) $mode $endpoint"
 $clientPid=[PackageActivator]::Activate($script:aumid,$arguments);if($clientPid -gt 0){$script:activatedClientPids.Add([uint32]$clientPid)}
 $deadline=(Get-Date).AddSeconds(15);$actual='';$content=''
 do{Start-Sleep -Milliseconds 100;$p=Get-Process -Id $clientPid -ErrorAction SilentlyContinue
  if($p){$w=Get-CimInstance Win32_Process -Filter "ProcessId=$clientPid" -ErrorAction SilentlyContinue;if($w.ExecutablePath){$actual=$w.ExecutablePath}}
  if(-not $actual){$m=Get-ClientLog $clientPid;if($m){$content=Get-Content $m.FullName -Raw;$pm=[regex]::Match($content,'RPC_CLIENT_PROCESS_IMAGE=(.+)');if($pm.Success){$actual=$pm.Groups[1].Value.Trim()}}}
 }while(-not $actual -and (Get-Date) -lt $deadline)
 $expected=Join-Path $script:package.InstallLocation 'client.exe';Write-Host "ACTIVATED_PID=$clientPid OBSERVED_IMAGE=$actual EXPECTED_IMAGE=$expected"
 if(-not $actual -or $actual -ine $expected){throw "Activated process path mismatch or missing: pid=$clientPid actual=$actual log=$content"}
 return [uint32]$clientPid
}
function Invoke-PackagedCase([string]$case,[string]$mode='rpc'){
 $s=Start-RpcServer $case;$clientPid=Activate-Client $mode $s.Endpoint
 $marker=if($mode -eq 'begin-only'){'RPC_CLIENT_BEGIN_ONLY=PASS'}else{'RPC_CLIENT_CALL_STATUS='}
 $cl=Wait-ClientLog $clientPid $marker;$sv=Finish-RpcServer $s
 $nonce=[regex]::Match($cl.Content,'RPC_SESSION_NONCE=([0-9a-f]{32})').Groups[1].Value
 return @{Pid=$clientPid;Client=$cl.Content;Server=$sv.Content;Endpoint=$s.Endpoint;Nonce=$nonce}
}
function Invoke-CommonCase([string]$case,[string]$mode){
 $s=Start-RpcServer $case;$out=Join-Path $root "$case.common.log";$err=Join-Path $root "$case.common.err"
 $a=@('--rpc-client',$script:package.PackageFamilyName,$script:expectedSid,$script:package.PackageFullName,$mode,$s.Endpoint)|ForEach-Object {'"'+($_ -replace '"','\"')+'"'}
 $p=Start-Process (Join-Path $root 'common.exe') -ArgumentList ($a -join ' ') -PassThru -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError $err
 if(-not $p.WaitForExit(20000)){try{$p.Kill()}catch{};throw "Common RPC client timeout $case"}
 $cl=Get-Content $out -Raw -ErrorAction SilentlyContinue;$sv=Finish-RpcServer $s
 Write-Host "COMMON_CLIENT_EXIT=$($p.ExitCode)";Write-Host $cl
 return @{Pid=$p.Id;Client=$cl;Server=$sv.Content;Endpoint=$s.Endpoint}
}
try{
 # Resume guard: rounds 01-14 were recorded in Run 43901240165; do not execute or recount them.
 $current=(git branch --show-current).Trim();if($current -ne $branch){throw "Wrong branch: $current"}
 $head=(git rev-parse HEAD).Trim();git merge-base --is-ancestor 'e837055c7a3263d2cae797a35a40b684b32a1716' HEAD;if($LASTEXITCODE -ne 0){throw "Audited resume HEAD is not an ancestor of $head"};Write-Host "RESUME_COMMIT=$head AUDITED_RESUME_HEAD=e837055c7a3263d2cae797a35a40b684b32a1716"
 git merge-base --is-ancestor $baseline HEAD;if($LASTEXITCODE -ne 0){throw 'Baseline ancestry failed'}
 $status=(git status --porcelain);if($status){throw "Dirty checkout: $status"}
 $changed=@(git diff --name-only "$baseline..HEAD");$unexpected=@($changed|Where-Object {$_ -notlike 'scripts/experimental/*' -and $_ -ne '.github/workflows/phone-02-windows-identity-poa.yml'});if($unexpected.Count){throw "Unexpected production file changes: $unexpected"}
 $flags.ProductionUntouched=$true
 Write-Host "RESUME_BRANCH=$current RESUME_HEAD=$head BASELINE=$baseline"
 Write-Host 'ROUNDS_01_14=PREVIOUSLY_RECORDED_IN_RUN_43901240165_NOT_REPEATED'
 Write-Host "BASELINE_DIFF=$($changed -join ';') PRODUCTION_FILES_CHANGED=False"
 $pf86=[Environment]::GetEnvironmentVariable('ProgramFiles(x86)');$sdkRoot=Join-Path $pf86 'Windows Kits\10\bin';$sdk='10.0.26100.0';$makeappx=Join-Path $sdkRoot "$sdk\x64\MakeAppx.exe";$signtool=Join-Path $sdkRoot "$sdk\x64\SignTool.exe";$midl=Join-Path $sdkRoot "$sdk\x64\midl.exe";$vswhere=Join-Path $pf86 'Microsoft Visual Studio\Installer\vswhere.exe';$vs=(& $vswhere -latest -products '*' -property installationPath).Trim();$vcvars=Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat';if(-not(Test-Path $midl)-or-not(Test-Path $makeappx)-or-not(Test-Path $signtool)-or-not(Test-Path $vcvars)){throw 'Required SDK/VS/MIDL tools missing'};Write-Host "MIDL=$midl VC_VARS=$vcvars"
 Write-Host "MIDL_FILE_VERSION=$((Get-Item $midl).VersionInfo.FileVersion)"
 $idl=Join-Path $env:GITHUB_WORKSPACE 'scripts\experimental\phone02-msix-rpc.idl';$source=Join-Path $env:GITHUB_WORKSPACE 'scripts\experimental\phone02-msix-rpc.cpp';Push-Location $rpcDir;try{$midlCmd='"'+$vcvars+'" && "'+$midl+'" /nologo /env x64 /h phone02-msix-rpc.h /cstub phone02-msix-rpc_c.c /sstub phone02-msix-rpc_s.c "'+$idl+'"';& cmd.exe /d /s /c $midlCmd;if($LASTEXITCODE -ne 0){throw "MIDL exit=$LASTEXITCODE"}}finally{Pop-Location}

 Round 15 'validate actual MIDL header and client/server RPC stubs';$required=@('phone02-msix-rpc.h','phone02-msix-rpc_c.c','phone02-msix-rpc_s.c');foreach($n in $required){$p=Join-Path $rpcDir $n;if(-not(Test-Path $p)-or(Get-Item $p).Length -le 0){throw "Missing or empty required MIDL output $n"}};$header=Get-Content (Join-Path $rpcDir 'phone02-msix-rpc.h') -Raw;$clientStub=Get-Content (Join-Path $rpcDir 'phone02-msix-rpc_c.c') -Raw;$serverStub=Get-Content (Join-Path $rpcDir 'phone02-msix-rpc_s.c') -Raw;foreach($symbol in @('Phone02BeginSession','Phone02Authorize')){if($header -notmatch $symbol -or $clientStub -notmatch $symbol -or $serverStub -notmatch $symbol){throw "RPC symbol missing from generated header/client/server stubs: $symbol"}};Write-Host 'MIDL_OUTPUTS=phone02-msix-rpc.h;phone02-msix-rpc_c.c;phone02-msix-rpc_s.c';Write-Host 'MIDL_IID_OUTPUT=NOT_REQUIRED_FOR_THIS_RPC_INTERFACE';Write-Host 'MIDL_CLIENT_SERVER_STUBS=PASS'
 Round 16 'compile distinct RPC client and supervisor executables';$supervisorExe=Join-Path $root 'supervisor.exe';$clientExe=Join-Path $root 'client.exe';$commonExe=Join-Path $root 'common.exe';$serverCmd='"'+$vcvars+'" && cl /nologo /EHsc /W4 /std:c++20 /DUNICODE /D_UNICODE /DPHONE02_RPC_SERVER_ONLY /I"'+$rpcDir+'" "'+$source+'" "'+(Join-Path $rpcDir 'phone02-msix-rpc_s.c')+'" /Fe:"'+$supervisorExe+'" rpcrt4.lib bcrypt.lib advapi32.lib userenv.lib kernel32.lib';& cmd.exe /d /s /c $serverCmd;if($LASTEXITCODE -ne 0){throw "RPC supervisor compile exit=$LASTEXITCODE"};$clientCmd='"'+$vcvars+'" && cl /nologo /EHsc /W4 /std:c++20 /DUNICODE /D_UNICODE /DPHONE02_RPC_CLIENT_ONLY /I"'+$rpcDir+'" "'+$source+'" "'+(Join-Path $rpcDir 'phone02-msix-rpc_c.c')+'" /Fe:"'+$clientExe+'" rpcrt4.lib bcrypt.lib advapi32.lib userenv.lib kernel32.lib';& cmd.exe /d /s /c $clientCmd;if($LASTEXITCODE -ne 0){throw "RPC client compile exit=$LASTEXITCODE"};Copy-Item $clientExe $commonExe -Force;if(-not(Test-Path $supervisorExe)-or-not(Test-Path $clientExe)-or-not(Test-Path $commonExe)){throw 'Role-specific RPC executable missing'};Write-Host 'RPC_ROLE_BUILDS=PASS'
 Round 17 'verify role-specific executable identities and package client';$serverHash=(Get-FileHash $supervisorExe -Algorithm SHA256).Hash;$clientHash=(Get-FileHash $clientExe -Algorithm SHA256).Hash;if($serverHash -eq $clientHash){throw 'Supervisor and client binaries unexpectedly identical'};Copy-Item $clientExe (Join-Path $pkg 'client.exe') -Force;$packedSourceHash=(Get-FileHash (Join-Path $pkg 'client.exe') -Algorithm SHA256).Hash;if($packedSourceHash -ne $clientHash){throw 'Packaged client copy hash mismatch'};Write-Host "RPC_SUPERVISOR_SHA256=$serverHash RPC_CLIENT_SHA256=$clientHash"
 Round 18 'create minimal AppContainer-only manifest';$publisher='CN=TokenSaver Phone02 P234 RPC TEST ONLY';$manifest=@"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10" xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10" IgnorableNamespaces="uap uap10">
<Identity Name="TokenSaver.Phone02.IdentityPoA" Publisher="$publisher" Version="1.0.0.0" ProcessorArchitecture="x64"/>
<Properties><DisplayName>Phone02 P2.34 RPC Identity PoA TEST ONLY</DisplayName><PublisherDisplayName>TEST ONLY</PublisherDisplayName><Logo>Assets/StoreLogo.png</Logo></Properties>
<Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0"/></Dependencies>
<Resources><Resource Language="en-us"/></Resources>
<Applications><Application Id="App" Executable="client.exe" EntryPoint="Windows.PartialTrustApplication" uap10:TrustLevel="appContainer" uap10:RuntimeBehavior="packagedClassicApp"><uap:VisualElements DisplayName="Phone02 P2.34 RPC Client" Description="Disposable AppContainer RPC test" BackgroundColor="transparent" Square150x150Logo="Assets/Square150x150Logo.png" Square44x44Logo="Assets/Square44x44Logo.png"/></Application></Applications><Capabilities/>
</Package>
"@;Set-Content (Join-Path $pkg 'AppxManifest.xml') $manifest -Encoding utf8
 Write-Host 'PACKAGE_LOGO_ASSETS_CREATE=START';New-Item -ItemType Directory -Force (Join-Path $pkg 'Assets')|Out-Null;Add-Type -AssemblyName System.Drawing;foreach($n in @('StoreLogo.png','Square150x150Logo.png','Square44x44Logo.png')){$bmp=New-Object System.Drawing.Bitmap 64,64;$bmp.Save((Join-Path $pkg "Assets\$n"),[System.Drawing.Imaging.ImageFormat]::Png);$bmp.Dispose()}
 Write-Host 'TEST_SIGNING_CERT_CREATE=START';$cert=New-SelfSignedCertificate -Type Custom -Subject $publisher -KeyUsage DigitalSignature -FriendlyName 'TEST ONLY Phone02 P2.34 RPC' -CertStoreLocation 'Cert:\CurrentUser\My' -KeyAlgorithm RSA -KeyLength 2048 -HashAlgorithm SHA256 -NotAfter (Get-Date).AddDays(2);$certPath=Join-Path $root 'test-only.cer';Export-Certificate -Cert $cert -FilePath $certPath|Out-Null
 Round 19 'trust certificate, package, sign, install and derive package identity';RunTool (Join-Path $env:SystemRoot 'System32\certutil.exe') ('-user -f -addstore TrustedPeople "'+$certPath+'"') 'cert-user-trustedpeople' 30;RunTool (Join-Path $env:SystemRoot 'System32\certutil.exe') ('-f -addstore TrustedPeople "'+$certPath+'"') 'cert-machine-trustedpeople' 30;$userTrusted=@(Get-ChildItem Cert:\CurrentUser\TrustedPeople -ErrorAction SilentlyContinue|Where-Object Thumbprint -eq $cert.Thumbprint);$machineTrusted=@(Get-ChildItem Cert:\LocalMachine\TrustedPeople -ErrorAction SilentlyContinue|Where-Object Thumbprint -eq $cert.Thumbprint);if($userTrusted.Count -lt 1 -or $machineTrusted.Count -lt 1){throw "Certificate trust import not verified: CurrentUser=$($userTrusted.Count) LocalMachine=$($machineTrusted.Count)"};Write-Host "CERT_TRUST_IMPORT_VERIFIED=TRUE THUMBPRINT=$($cert.Thumbprint) CURRENTUSER_MATCHES=$($userTrusted.Count) LOCALMACHINE_MATCHES=$($machineTrusted.Count)";foreach($entry in $userTrusted){Write-Host "CERT_USER_ENTRY=$($entry.PSPath) SUBJECT=$($entry.Subject) SERIAL=$($entry.SerialNumber)"};foreach($entry in $machineTrusted){Write-Host "CERT_MACHINE_ENTRY=$($entry.PSPath) SUBJECT=$($entry.Subject) SERIAL=$($entry.SerialNumber)"}
 $msix=Join-Path $root 'Phone02P234RpcPoA.msix';RunTool $makeappx ('pack /d "'+$pkg+'" /p "'+$msix+'" /o') 'makeappx-pack' 120
 $unpack=Join-Path $root 'unpacked';New-Item -ItemType Directory -Force $unpack|Out-Null;RunTool $makeappx ('unpack /p "'+$msix+'" /d "'+$unpack+'"') 'makeappx-unpack' 120;if(-not(Test-Path (Join-Path $unpack 'AppxManifest.xml'))-or-not(Test-Path (Join-Path $unpack 'client.exe'))){throw 'Unpacked package incomplete'}
 $packedHash=(Get-FileHash (Join-Path $unpack 'client.exe') -Algorithm SHA256).Hash;if($packedHash -ne $clientHash){throw "Client hash mismatch: unpacked=$packedHash compiled=$clientHash"};Write-Host "CLIENT_SHA256=$packedHash COMPILED_CLIENT_SHA256=$clientHash"
 RunTool $signtool ('sign /fd SHA256 /sha1 '+$cert.Thumbprint+' "'+$msix+'"') 'signtool-sign' 120
 RunTool $signtool ('verify /pa /v "'+$msix+'"') 'signtool-verify' 120;Write-Host "MSIX_SHA256=$((Get-FileHash $msix -Algorithm SHA256).Hash) CERT=$($cert.Thumbprint)"
 $install="Add-AppxPackage -Path '"+$msix.Replace("'","''")+"' -ForceUpdateFromAnyVersion -ErrorAction Stop";$enc=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($install));RunTool (Join-Path $PSHOME 'pwsh.exe') ("-NoLogo -NoProfile -NonInteractive -EncodedCommand "+$enc) 'add-appxpackage' 180;$package=Get-AppxPackage -Name 'TokenSaver.Phone02.IdentityPoA';if(-not $package){throw 'Package not registered'};$script:package=$package
 Write-Host "PACKAGE_FAMILY=$($package.PackageFamilyName) PACKAGE_FULL_NAME=$($package.PackageFullName) INSTALL_LOCATION=$($package.InstallLocation)"
 Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public static class Phone02SidApi{[DllImport("userenv.dll",CharSet=CharSet.Unicode)]public static extern int DeriveAppContainerSidFromAppContainerName(string n,out IntPtr sid);[DllImport("advapi32.dll",CharSet=CharSet.Unicode)]public static extern bool ConvertSidToStringSid(IntPtr sid,out IntPtr text);[DllImport("kernel32.dll")]public static extern IntPtr LocalFree(IntPtr h);}' -PassThru|Out-Null;$sidPtr=[IntPtr]::Zero;$rc=[Phone02SidApi]::DeriveAppContainerSidFromAppContainerName($package.PackageFamilyName,[ref]$sidPtr);if($rc -ne 0){throw "SID derivation failed $rc"};$textPtr=[IntPtr]::Zero;if(-not[Phone02SidApi]::ConvertSidToStringSid($sidPtr,[ref]$textPtr)){throw 'SID stringify failed'};$expectedSid=[Runtime.InteropServices.Marshal]::PtrToStringUni($textPtr);[Phone02SidApi]::LocalFree($textPtr)|Out-Null;[Phone02SidApi]::LocalFree($sidPtr)|Out-Null;$script:expectedSid=$expectedSid;Write-Host "EXPECTED_APPCONTAINER_SID=$expectedSid"
 $script:aumid="$($package.PackageFamilyName)!App";Write-Host "AUMID=$script:aumid"
 Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
[ComImport,Guid("2e941141-7f97-4756-ba1d-9decde894a3d"),InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IApplicationActivationManager{[PreserveSig]int ActivateApplication([MarshalAs(UnmanagedType.LPWStr)]string appUserModelId,[MarshalAs(UnmanagedType.LPWStr)]string arguments,uint options,out uint processId);}
[ComImport,Guid("45BA127D-10A8-46EA-8AB7-56EA9078943C"),ClassInterface(ClassInterfaceType.None)]public class ApplicationActivationManager{}
public static class PackageActivator{[DllImport("ole32.dll")]private static extern int CoInitializeEx(IntPtr p,uint flags);[DllImport("ole32.dll")]private static extern void CoUninitialize();public static uint Activate(string id,string args){int hr=CoInitializeEx(IntPtr.Zero,2);if(hr<0&&hr!=unchecked((int)0x80010106))Marshal.ThrowExceptionForHR(hr);IApplicationActivationManager m=null;try{m=(IApplicationActivationManager)new ApplicationActivationManager();uint pid;hr=m.ActivateApplication(id,args,0,out pid);if(hr<0)Marshal.ThrowExceptionForHR(hr);return pid;}finally{if(m!=null&&Marshal.IsComObject(m))Marshal.ReleaseComObject(m);if(hr>=0)CoUninitialize();}}}
'@
 Round 20 'start supervisor, activate MSIX client and prove initial ncalrpc call';$server=Start-RpcServer 'legitimate'
 if((Get-Content $server.Out -Raw) -notmatch 'RPC_SERVER_READY=TRUE'){throw 'RPC server readiness marker missing'}
 $legitPid=Activate-Client 'rpc' $server.Endpoint
 $legitClient=Wait-ClientLog $legitPid 'RPC_CLIENT_CALL_STATUS=';if($legitClient.Content -notmatch 'RPC_CLIENT_BINDING=PASS'){throw 'RPC client binding failed'}
 $legitServer=Finish-RpcServer $server;if($legitServer.Content -notmatch "RPC_CLIENT_PID=$legitPid(\s|$)"){throw 'RPC observed PID mismatch'}
  Round 21 'audit compiler and generated stub outputs after correction';if(-not(Test-Path $supervisorExe)-or-not(Test-Path $clientExe)-or-not(Test-Path (Join-Path $rpcDir 'phone02-msix-rpc_c.c'))-or-not(Test-Path (Join-Path $rpcDir 'phone02-msix-rpc_s.c'))){throw 'Compiled RPC artifacts missing'};Write-Host 'ROUND21_BUILD_AUDIT=PASS'
  Round 22 'audit package signature and binary integrity after correction';if($packedHash -ne $clientHash -or -not(Test-Path $msix)){throw 'MSIX/client integrity audit failed'};Write-Host "ROUND22_PACKAGE_AUDIT=PASS MSIX_SHA256=$((Get-FileHash $msix -Algorithm SHA256).Hash)"
  Round 23 'audit Windows package registration and activation result';if(-not $package -or -not $legitPid -or $legitClient.Content -notmatch 'RPC_CLIENT_PROCESS_IMAGE='){throw 'MSIX activation evidence incomplete'};Write-Host 'ROUND23_ACTIVATION_AUDIT=PASS'
  Round 24 'audit real ncalrpc transport and security callback';if($legitServer.Content -notmatch 'RPC_CALL_RECEIVED=TRUE' -or $legitServer.Content -notmatch 'RPC_SECURITY_CALLBACK_RESULT=ALLOW' -or $legitServer.Content -notmatch 'RPC_LOCALITY=1'){throw 'ncalrpc transport/callback audit failed'};Write-Host 'ROUND24_NCALRPC_AUDIT=PASS'
  Round 25 'close correction gate without masking or unnecessary retries';if($legitClient.Content -match 'ERROR_FILE_NOT_FOUND|RPC_CLIENT_BEGIN_SESSION=FAIL' -or $legitServer.Content -match 'RPC_SECURITY_CALLBACK_RESULT=DENY'){throw 'Unresolved RPC correction remains'};Write-Host 'ROUND25_CORRECTION_GATE=PASS RETRY_REQUIRED=FALSE'
 Round 26 'verify supervisor-observed PID equals the activated Windows PID';if($legitServer.Content -notmatch "RPC_CLIENT_PID=$legitPid(\s|$)"){throw 'Activated PID and RPC runtime PID differ'};Write-Host "LEGITIMATE_PID_CORRELATION=PASS PID=$legitPid"
  Round 27 'verify AppContainer flag in process and impersonation tokens';if($legitServer.Content -notmatch 'TOKEN_IS_APPCONTAINER=1 THREAD_TOKEN_IS_APPCONTAINER=1'){throw 'AppContainer token not observed'}
 Round 28 'verify Package Family and Full Name from process handle';if($legitServer.Content -notmatch [regex]::Escape("OBSERVED_PACKAGE_FAMILY=$($package.PackageFamilyName)") -or $legitServer.Content -notmatch [regex]::Escape("OBSERVED_PACKAGE_FULL_NAME=$($package.PackageFullName)")){throw 'Package identity mismatch'}
 Round 29 'verify PID/process/token correlation, session and user';if($legitServer.Content -notmatch 'PID_PROCESS_TOKEN_CORRELATION=PASS' -or $legitServer.Content -notmatch 'PROCESS_PATH_MATCH=PASS' -or $legitServer.Content -notmatch 'SESSION_MATCH=PASS' -or $legitServer.Content -notmatch 'USER_MATCH=PASS'){throw 'Token/process correlation failed'}
 Round 30 'require nonce-bound legitimate ALLOW';if($legitServer.Content -notmatch 'NONCE_MATCH=PASS NONCE_PID_MATCH=PASS NONCE_ONE_TIME=PASS' -or $legitServer.Content -notmatch 'AUTHORIZATION_RESULT=ALLOW' -or $legitClient.Content -notmatch 'RPC_CLIENT_AUTHORIZATION=ALLOW'){throw 'Legitimate RPC authorization failed'};$flags.Legitimate=$true;$oldNonce=[regex]::Match($legitClient.Content,'RPC_SESSION_NONCE=([0-9a-f]{32})').Groups[1].Value;if(-not $oldNonce){throw 'No session nonce logged'}
 Round 31 'same-user common process presents forged but matching claims';$common=Invoke-CommonCase 'same-user-common' 'common';if($common.Server -notmatch 'TOKEN_IS_APPCONTAINER=0' -or $common.Server -notmatch 'CLIENT_CLAIMS_PRETEND_LEGIT=TRUE' -or $common.Server -notmatch 'CLIENT_CLAIMS_MATCH_OBSERVED=FALSE' -or $common.Server -notmatch 'AUTHORIZATION_RESULT=REJECT'){throw 'Same-user process did not present forged package claims or was not rejected'};$flags.CommonReject=$true
 Round 32 'prove same-user is not package identity';if($common.Server -notmatch 'PACKAGE_IDENTITY_MATCH=FAIL' -or $common.Server -notmatch 'PID_PROCESS_TOKEN_CORRELATION=PASS'){throw 'Adversary identity evidence incomplete'}
  Round 33 'verify same-user process and impersonation tokens are non-AppContainer';if($common.Server -notmatch 'TOKEN_IS_APPCONTAINER=0 THREAD_TOKEN_IS_APPCONTAINER=0'){throw 'Same-user token observation failed'}
  Round 34 'verify same-user package claims are forged and not OS-observed';if($common.Server -notmatch 'CLIENT_CLAIMS_PRETEND_LEGIT=TRUE' -or $common.Server -notmatch 'CLIENT_CLAIMS_MATCH_OBSERVED=FALSE'){throw 'Same-user forged claims not demonstrated'}
  Round 35 'prove same-user rejection occurs after RPC endpoint admission';if($common.Server -notmatch 'RPC_SECURITY_CALLBACK_RESULT=ALLOW' -or $common.Server -notmatch 'AUTHORIZATION_RESULT=REJECT'){throw 'Same-user token-validation rejection not proven'};Write-Host 'COMMON_REJECTION_LAYER=SUPERVISOR_TOKEN_AND_PACKAGE_VALIDATION;ENDPOINT_ACL=ALLOW'
 Round 36 'spoof client PID claim';$p1=Invoke-CommonCase 'spoof-pid' 'spoof-pid';if($p1.Server -notmatch 'CLIENT_CLAIMS_MATCH_OBSERVED=FALSE' -or $p1.Server -notmatch 'AUTHORIZATION_RESULT=REJECT'){throw 'PID spoof accepted'};$flags.SpoofPid=$true
  Round 37 'spoof AppContainer declaration';$p2=Invoke-CommonCase 'spoof-app' 'spoof-app';if($p2.Server -notmatch 'TOKEN_IS_APPCONTAINER=0' -or $p2.Server -notmatch 'AUTHORIZATION_RESULT=REJECT'){throw 'AppContainer declaration spoof accepted'};$flags.SpoofApp=$true
 Round 38 'spoof AppContainer SID';$s1=Invoke-CommonCase 'spoof-sid' 'spoof-sid';if($s1.Server -notmatch 'CLIENT_CLAIMS_MATCH_OBSERVED=FALSE' -or $s1.Server -notmatch 'AUTHORIZATION_RESULT=REJECT'){throw 'SID spoof accepted'};$flags.SpoofSid=$true
 Round 39 'spoof Package Family and Full Name';$f1=Invoke-CommonCase 'spoof-family' 'spoof-family';$f2=Invoke-CommonCase 'spoof-full' 'spoof-full';if($f1.Server -notmatch 'AUTHORIZATION_RESULT=REJECT' -or $f2.Server -notmatch 'AUTHORIZATION_RESULT=REJECT'){throw 'Package identity spoof accepted'};$flags.SpoofFamily=$true;$flags.SpoofFull=$true
 Round 40 'spoof all claims from actual packaged client';$sp=Invoke-PackagedCase 'packaged-spoof-all' 'spoof-all';if($sp.Server -notmatch 'TOKEN_IS_APPCONTAINER=1' -or $sp.Server -notmatch 'CLIENT_CLAIMS_MATCH_OBSERVED=FALSE' -or $sp.Server -notmatch 'AUTHORIZATION_RESULT=REJECT'){throw 'Packaged spoof claims were not rejected'};$flags.SpoofPackage=$true
 Round 41 'start two isolated supervisor endpoints for concurrent instances';$sA=Start-RpcServer 'instance-a';$sB=Start-RpcServer 'instance-b';if($sA.Endpoint -eq $sB.Endpoint){throw 'Concurrent instance endpoints collided'}
  Round 42 'activate two AppContainer clients concurrently';$pidA=Activate-Client 'rpc-delay' $sA.Endpoint;$pidB=Activate-Client 'rpc-delay' $sB.Endpoint;if($pidA -eq $pidB){throw 'Concurrent package activations reused a PID'};$cA=Wait-ClientLog $pidA 'RPC_CLIENT_CALL_STATUS=';$cB=Wait-ClientLog $pidB 'RPC_CLIENT_CALL_STATUS='
  Round 43 'prove per-instance PID, token, package and nonce binding';$rA=Finish-RpcServer $sA;$rB=Finish-RpcServer $sB;$nonceA=[regex]::Match($cA.Content,'RPC_SESSION_NONCE=([0-9a-f]{32})').Groups[1].Value;$nonceB=[regex]::Match($cB.Content,'RPC_SESSION_NONCE=([0-9a-f]{32})').Groups[1].Value;if($nonceA -eq $nonceB -or $rA.Content -notmatch "RPC_CLIENT_PID=$pidA(\s|$)" -or $rB.Content -notmatch "RPC_CLIENT_PID=$pidB(\s|$)" -or $rA.Content -notmatch 'TOKEN_IS_APPCONTAINER=1 THREAD_TOKEN_IS_APPCONTAINER=1' -or $rB.Content -notmatch 'TOKEN_IS_APPCONTAINER=1 THREAD_TOKEN_IS_APPCONTAINER=1' -or $rA.Content -notmatch 'AUTHORIZATION_RESULT=ALLOW' -or $rB.Content -notmatch 'AUTHORIZATION_RESULT=ALLOW'){throw 'Two package instances not independently correlated/authorized'};Write-Host "INSTANCE_PID_1=$pidA INSTANCE_PID_2=$pidB INSTANCE_NONCE_1=$nonceA INSTANCE_NONCE_2=$nonceB ENDPOINTS_DISTINCT=TRUE";$flags.TwoInstances=$true
 Round 44 'reconnect after process exit using new PID and nonce';$reconnect=Invoke-PackagedCase 'reconnect' 'rpc';if($reconnect.Pid -eq $legitPid -or $reconnect.Nonce -eq $oldNonce -or $reconnect.Server -notmatch 'AUTHORIZATION_RESULT=ALLOW' -or $reconnect.Server -notmatch 'NONCE_MATCH=PASS NONCE_PID_MATCH=PASS NONCE_ONE_TIME=PASS'){throw 'Reconnect authorization or fresh session binding failed'};$flags.Reconnect=$true
 Round 45 'replay old session nonce against new one-time server session';$replayServer=Start-RpcServer 'replay-target';$beginPid=Activate-Client 'begin-only' $replayServer.Endpoint;$begin=Wait-ClientLog $beginPid 'RPC_CLIENT_BEGIN_ONLY=PASS';$fresh=[regex]::Match($begin.Content,'RPC_SESSION_NONCE=([0-9a-f]{32})').Groups[1].Value;if(-not $fresh -or $fresh -eq $oldNonce){throw 'Fresh nonce absent or reused'};$out=Join-Path $root 'replay-client.log';$err=Join-Path $root 'replay-client.err';$ra=@('--rpc-replay',$package.PackageFamilyName,$expectedSid,$package.PackageFullName,$replayServer.Endpoint,$oldNonce)|ForEach-Object {'"'+($_ -replace '"','\"')+'"'};$rp=Start-Process (Join-Path $root 'common.exe') -ArgumentList ($ra -join ' ') -PassThru -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError $err;if(-not $rp.WaitForExit(20000)){try{$rp.Kill()}catch{};throw 'Replay client timeout'};$rr=Finish-RpcServer $replayServer;Write-Host (Get-Content $out -Raw -ErrorAction SilentlyContinue);if($rr.Content -notmatch 'NONCE_MATCH=FAIL' -or $rr.Content -notmatch 'NONCE_PID_MATCH=FAIL' -or $rr.Content -notmatch 'AUTHORIZATION_RESULT=REJECT'){throw 'Prior-session replay was not rejected'};$flags.Replay=$true
 Round 46 'initiate cleanup of package, certificate, client and supervisor processes'
}finally{
 foreach($p in $rpcServers){try{if(-not $p.HasExited){Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue;$p.WaitForExit(5000)|Out-Null}}catch{}}
 $expectedClientPath=$null;if($package){$expectedClientPath=Join-Path $package.InstallLocation 'client.exe'}
 foreach($clientPid in $activatedClientPids){try{$proc=Get-CimInstance Win32_Process -Filter "ProcessId=$clientPid" -ErrorAction SilentlyContinue;if($proc -and $expectedClientPath -and $proc.ExecutablePath -ieq $expectedClientPath){Stop-Process -Id $clientPid -Force -ErrorAction SilentlyContinue}}catch{}}
 Start-Sleep -Milliseconds 250
 $alive=@($rpcServers|Where-Object{try{-not $_.HasExited}catch{$false}})
 if(-not $package){$package=Get-AppxPackage -Name 'TokenSaver.Phone02.IdentityPoA' -ErrorAction SilentlyContinue}
 if($package){Remove-AppxPackage -Package $package.PackageFullName -ErrorAction SilentlyContinue}
 $remaining=Get-AppxPackage -Name 'TokenSaver.Phone02.IdentityPoA' -ErrorAction SilentlyContinue
 $clientRemaining=@();foreach($clientPid in $activatedClientPids){$proc=Get-CimInstance Win32_Process -Filter "ProcessId=$clientPid" -ErrorAction SilentlyContinue;if($proc -and $expectedClientPath -and $proc.ExecutablePath -ieq $expectedClientPath){$clientRemaining+=$clientPid}}
 Write-Host "CLEANUP_PACKAGE_REMAINS=$([bool]$remaining) RPC_SUPERVISORS_REMAINS=$($alive.Count) CLIENT_PROCESSES_REMAINS=$($clientRemaining.Count)"
 $certRemaining=$false
 if($cert){foreach($store in @('Cert:\CurrentUser\My','Cert:\CurrentUser\Root','Cert:\CurrentUser\TrustedPeople','Cert:\LocalMachine\TrustedPeople')){Remove-Item "$store\$($cert.Thumbprint)" -ErrorAction SilentlyContinue};$left=@(Get-ChildItem Cert:\CurrentUser\My,Cert:\CurrentUser\Root,Cert:\CurrentUser\TrustedPeople,Cert:\LocalMachine\TrustedPeople -ErrorAction SilentlyContinue|Where-Object Thumbprint -eq $cert.Thumbprint);$certRemaining=$left.Count -gt 0;Write-Host "CLEANUP_CERT_REMAINS=$certRemaining"}
 $changes=@(git diff --name-only "$baseline..HEAD");$unexpected=@($changes|Where-Object{$_ -notlike 'scripts/experimental/*' -and $_ -ne '.github/workflows/phone-02-windows-identity-poa.yml'});Write-Host "FINAL_BASELINE_DIFF=$($changes -join ';')";Write-Host "PRODUCTION_FILES_CHANGED=$($unexpected.Count -gt 0)"
 $flags.Cleanup=($alive.Count -eq 0)-and(-not $remaining)-and($clientRemaining.Count -eq 0)-and(-not $certRemaining);$flags.ProductionUntouched=($unexpected.Count -eq 0)
 Write-Host "CLEANUP_COMPLETE=$($flags.Cleanup)"
 try{Stop-Transcript|Out-Null}catch{}
}
Round 47 'verify package, certificate, client and supervisor cleanup';if(-not $flags.Cleanup){throw 'Cleanup audit failed'};Write-Host 'CLEANUP_VERIFIED=TRUE PACKAGE_REMAINS=FALSE CERT_REMAINS=FALSE CLIENT_PROCESSES_REMAINS=0 SUPERVISORS_REMAINS=0'
Round 48 'security audit of ACL, callback, token correlation and authorization policy';$sourceText=Get-Content $source -Raw;if(-not $sourceText.Contains('Phone02SecurityCallback,sd') -or -not $sourceText.Contains('RPC_QUERY_CLIENT_PID|RPC_QUERY_IS_CLIENT_LOCAL') -or -not $sourceText.Contains('RPC_IF_ALLOW_SECURE_ONLY|RPC_IF_ALLOW_LOCAL_ONLY') -or -not $sourceText.Contains('processApp==1&&threadApp==1') -or -not $sourceText.Contains('nonceMatch&&noncePidMatch&&nonceUnused') -or -not $sourceText.Contains('D:P(A;;GA;;;')){throw 'Security policy source audit failed'};if($legitServer.Content -notmatch 'RPC_SECURITY_CALLBACK_RESULT=ALLOW' -or $common.Server -notmatch 'RPC_SECURITY_CALLBACK_RESULT=ALLOW' -or $common.Server -notmatch 'TOKEN_IS_APPCONTAINER=0' -or $common.Server -notmatch 'AUTHORIZATION_RESULT=REJECT'){throw 'Security behavior audit failed'};$flags.SecurityAudit=$true;Write-Host 'SECURITY_AUDIT=PASS COMMON_REJECTION_LAYER=SUPERVISOR_TOKEN_AND_PACKAGE_VALIDATION'
Round 49 'audit evidence and classify implementation, tests and proof';$requiredEvidence=@('p234-transcript.log','Phone02P234RpcPoA.msix','client.exe','common.exe','supervisor.exe','rpc\phone02-msix-rpc.h','rpc\phone02-msix-rpc_c.c','rpc\phone02-msix-rpc_s.c','legitimate.server.log','same-user-common.server.log','replay-target.server.log');foreach($rel in $requiredEvidence){if(-not(Test-Path (Join-Path $root $rel))){throw "Evidence artifact missing: $rel"}};$evidenceFiles=@(Get-ChildItem $root -File -Recurse);$summary=@'
P2.34 — WINDOWS RPC IDENTITY PROOF OF AUTHORIZATION
IMPLEMENTED: separate RPC client/supervisor binaries; AppContainer-aware endpoint and interface ACLs; local/authenticated RPC callback; server-side process/token/package correlation; one-time nonce/session binding.
TESTED: MSIX pack/sign/install/activation; ncalrpc; common same-user rejection; PID/AppContainer/SID/Family/FullName/all-claims spoofing; two instances; reconnect; replay; cleanup.
OBSERVED: Windows process PID, image path, process token, impersonation token, AppContainer SID, Package Family and Package Full Name.
PROVEN: legitimate AppContainer RPC ALLOW; same-user full-trust and spoof attempts REJECT; independent instance correlation; replay REJECT; disposable resources cleaned.
NOT PROVEN: Phone-02 production activation, production deployment, production credentials or production approval. Phone-02 production remains NOT ACTIVATED and NOT APPROVED.
'@;Set-Content (Join-Path $root 'p234-final-summary.txt') $summary -Encoding utf8;$flags.EvidenceAudit=$true;Write-Host "EVIDENCE_AUDIT=PASS FILE_COUNT=$($evidenceFiles.Count) REQUIRED=$($requiredEvidence.Count) MSIX_SHA256=$((Get-FileHash $msix -Algorithm SHA256).Hash)"
Round 50 'final classification and evidence summary'
$failed=@($flags.GetEnumerator()|Where-Object{-not $_.Value})
if($failed.Count -eq 0){$finalClassification='PASS'}elseif($flags.Legitimate -or $flags.CommonReject){$finalClassification='NO-GO'}else{$finalClassification='BLOCKED'}
Write-Host "P2.34_CLASSIFICATION=$finalClassification"
foreach($k in $flags.Keys){Write-Host "CONTROL_$($k.ToUpper())=$($flags[$k])"}
Write-Host "FINAL_HEAD=$((git rev-parse HEAD).Trim()) BASELINE=$baseline BRANCH=$branch"
if($finalClassification -ne 'PASS'){throw "P2.34 incomplete: $finalClassification; failed controls=$($failed.Name -join ',')"}
