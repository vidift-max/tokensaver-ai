$ErrorActionPreference='Stop'
$baseline='3b614efc7ce4b1a83e05c94d55c42c0cf9c04cb1'
$repo='vidift-max/tokensaver-ai';$branch='security/phone-02-windows-identity-poa'
$root=Join-Path $env:RUNNER_TEMP 'phone02-p234-rpc-evidence';$pkg=Join-Path $root 'package';$rpcDir=Join-Path $root 'rpc'
New-Item -ItemType Directory -Force -Path $pkg,$rpcDir|Out-Null
Start-Transcript -Path (Join-Path $root 'p234-transcript.log') -Force|Out-Null
$cert=$null;$package=$null;$finalClassification='BLOCKED'
$rpcServers=[System.Collections.Generic.List[System.Diagnostics.Process]]::new()
$flags=[ordered]@{Legitimate=$false;CommonReject=$false;SpoofPid=$false;SpoofApp=$false;SpoofSid=$false;SpoofFamily=$false;SpoofFull=$false;SpoofPackage=$false;TwoInstances=$false;Reconnect=$false;Replay=$false;Cleanup=$false;ProductionUntouched=$false}
function Round([int]$n,[string]$d){Write-Host ("ROUND_{0:D2} — {1}" -f $n,$d)}
function RunTool([string]$exe,[string]$args,[string]$label,[int]$seconds=120){
 $o=Join-Path $root "$label.out.log";$e=Join-Path $root "$label.err.log"
 $p=Start-Process -FilePath $exe -ArgumentList $args -PassThru -NoNewWindow -RedirectStandardOutput $o -RedirectStandardError $e
 if(-not $p.WaitForExit($seconds*1000)){try{$p.Kill()}catch{};throw "$label timeout"}
 if(Test-Path $o){Get-Content $o -Raw|Write-Host};if(Test-Path $e){Get-Content $e -Raw|Write-Host}
 if($p.ExitCode -ne 0){throw "$label failed exit=$($p.ExitCode)"}
}
function Get-ClientLog([uint32]$pid){
 $name="Phone02-MsixPoA-rpc-client-$pid.log";$base=Join-Path $env:LOCALAPPDATA "Packages\$($script:package.PackageFamilyName)"
 $m=$null;if(Test-Path $base){$m=Get-ChildItem $base -Filter $name -File -Recurse -ErrorAction SilentlyContinue|Select-Object -First 1}
 if(-not $m){$m=Get-ChildItem $env:LOCALAPPDATA -Filter $name -File -Recurse -ErrorAction SilentlyContinue|Select-Object -First 1}
 return $m
}
function Wait-ClientLog([uint32]$pid,[string]$marker,[int]$seconds=25){
 $deadline=(Get-Date).AddSeconds($seconds);$m=$null;$content=''
 do{Start-Sleep -Milliseconds 150;$m=Get-ClientLog $pid;if($m){$content=Get-Content $m.FullName -Raw -ErrorAction SilentlyContinue}}while(($content -notmatch [regex]::Escape($marker)) -and (Get-Date) -lt $deadline)
 if(-not $m -or $content -notmatch [regex]::Escape($marker)){throw "Client log marker missing: pid=$pid marker=$marker content=$content"}
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
 $pid=[PackageActivator]::Activate($script:aumid,$arguments)
 $deadline=(Get-Date).AddSeconds(15);$actual='';$content=''
 do{Start-Sleep -Milliseconds 100;$p=Get-Process -Id $pid -ErrorAction SilentlyContinue
  if($p){$w=Get-CimInstance Win32_Process -Filter "ProcessId=$pid" -ErrorAction SilentlyContinue;if($w.ExecutablePath){$actual=$w.ExecutablePath}}
  if(-not $actual){$m=Get-ClientLog $pid;if($m){$content=Get-Content $m.FullName -Raw;$pm=[regex]::Match($content,'RPC_CLIENT_PROCESS_IMAGE=(.+)');if($pm.Success){$actual=$pm.Groups[1].Value.Trim()}}}
 }while(-not $actual -and (Get-Date) -lt $deadline)
 $expected=Join-Path $script:package.InstallLocation 'client.exe';Write-Host "ACTIVATED_PID=$pid OBSERVED_IMAGE=$actual EXPECTED_IMAGE=$expected"
 if(-not $actual -or $actual -ine $expected){throw "Activated process path mismatch or missing: pid=$pid actual=$actual log=$content"}
 return [uint32]$pid
}
function Invoke-PackagedCase([string]$case,[string]$mode='rpc'){
 $s=Start-RpcServer $case;$pid=Activate-Client $mode $s.Endpoint
 $marker=if($mode -eq 'begin-only'){'RPC_CLIENT_BEGIN_ONLY=PASS'}else{'RPC_CLIENT_CALL_STATUS='}
 $cl=Wait-ClientLog $pid $marker;$sv=Finish-RpcServer $s
 $nonce=[regex]::Match($cl.Content,'RPC_SESSION_NONCE=([0-9a-f]{32})').Groups[1].Value
 return @{Pid=$pid;Client=$cl.Content;Server=$sv.Content;Endpoint=$s.Endpoint;Nonce=$nonce}
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
 Round 16 'compile RPC client, supervisor and adversary';$harness=Join-Path $root 'phone02-rpc-harness.exe';$cmd='"'+$vcvars+'" && cl /nologo /EHsc /W4 /std:c++20 /DUNICODE /D_UNICODE /I"'+$rpcDir+'" "'+$source+'" "'+(Join-Path $rpcDir 'phone02-msix-rpc_c.c')+'" "'+(Join-Path $rpcDir 'phone02-msix-rpc_s.c')+'"  /Fe:"'+$harness+'" rpcrt4.lib bcrypt.lib advapi32.lib userenv.lib kernel32.lib';& cmd.exe /d /s /c $cmd;if($LASTEXITCODE -ne 0){throw "RPC compile exit=$LASTEXITCODE"}
 Round 17 'create role-specific executable copies';Copy-Item $harness (Join-Path $root 'supervisor.exe') -Force;Copy-Item $harness (Join-Path $root 'common.exe') -Force;Copy-Item $harness (Join-Path $pkg 'client.exe') -Force;$binaryHash=(Get-FileHash $harness -Algorithm SHA256).Hash;Write-Host "RPC_HARNESS_SHA256=$binaryHash"
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
 Round 19 'create disposable package logo assets';New-Item -ItemType Directory -Force (Join-Path $pkg 'Assets')|Out-Null;Add-Type -AssemblyName System.Drawing;foreach($n in @('StoreLogo.png','Square150x150Logo.png','Square44x44Logo.png')){$bmp=New-Object System.Drawing.Bitmap 64,64;$bmp.Save((Join-Path $pkg "Assets\$n"),[System.Drawing.Imaging.ImageFormat]::Png);$bmp.Dispose()}
 Round 20 'create disposable signing certificate';$cert=New-SelfSignedCertificate -Type Custom -Subject $publisher -KeyUsage DigitalSignature -FriendlyName 'TEST ONLY Phone02 P2.34 RPC' -CertStoreLocation 'Cert:\CurrentUser\My' -KeyAlgorithm RSA -KeyLength 2048 -HashAlgorithm SHA256 -NotAfter (Get-Date).AddDays(2);$certPath=Join-Path $root 'test-only.cer';Export-Certificate -Cert $cert -FilePath $certPath|Out-Null
 Round 21 'trust disposable certificate for MSIX install';RunTool (Join-Path $env:SystemRoot 'System32\certutil.exe') ('-user -f -addstore TrustedPeople "'+$certPath+'"') 'cert-user-trustedpeople' 30;RunTool (Join-Path $env:SystemRoot 'System32\certutil.exe') ('-f -addstore TrustedPeople "'+$certPath+'"') 'cert-machine-trustedpeople' 30
 Round 22 'pack MSIX';$msix=Join-Path $root 'Phone02P234RpcPoA.msix';RunTool $makeappx ('pack /d "'+$pkg+'" /p "'+$msix+'" /o') 'makeappx-pack' 120
 Round 23 'unpack MSIX and verify manifest/client';$unpack=Join-Path $root 'unpacked';New-Item -ItemType Directory -Force $unpack|Out-Null;RunTool $makeappx ('unpack /p "'+$msix+'" /d "'+$unpack+'"') 'makeappx-unpack' 120;if(-not(Test-Path (Join-Path $unpack 'AppxManifest.xml'))-or-not(Test-Path (Join-Path $unpack 'client.exe'))){throw 'Unpacked package incomplete'}
 Round 24 'compare client binary SHA-256';$packedHash=(Get-FileHash (Join-Path $unpack 'client.exe') -Algorithm SHA256).Hash;if($packedHash -ne $binaryHash){throw 'Client hash mismatch'};Write-Host "CLIENT_SHA256=$packedHash"
 Round 25 'sign MSIX';RunTool $signtool ('sign /fd SHA256 /sha1 '+$cert.Thumbprint+' "'+$msix+'"') 'signtool-sign' 120
 Round 26 'verify Authenticode signature';RunTool $signtool ('verify /pa /v "'+$msix+'"') 'signtool-verify' 120;Write-Host "MSIX_SHA256=$((Get-FileHash $msix -Algorithm SHA256).Hash) CERT=$($cert.Thumbprint)"
 Round 27 'install test-only package';$install="Add-AppxPackage -Path '"+$msix.Replace("'","''")+"' -ForceUpdateFromAnyVersion -ErrorAction Stop";$enc=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($install));RunTool (Join-Path $PSHOME 'pwsh.exe') ("-NoLogo -NoProfile -NonInteractive -EncodedCommand "+$enc) 'add-appxpackage' 180;$package=Get-AppxPackage -Name 'TokenSaver.Phone02.IdentityPoA';if(-not $package){throw 'Package not registered'};$script:package=$package
 Round 28 'observe installed package family/full identity';Write-Host "PACKAGE_FAMILY=$($package.PackageFamilyName) PACKAGE_FULL_NAME=$($package.PackageFullName) INSTALL_LOCATION=$($package.InstallLocation)"
 Round 29 'derive expected AppContainer SID';Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public static class Phone02SidApi{[DllImport("userenv.dll",CharSet=CharSet.Unicode)]public static extern int DeriveAppContainerSidFromAppContainerName(string n,out IntPtr sid);[DllImport("advapi32.dll",CharSet=CharSet.Unicode)]public static extern bool ConvertSidToStringSid(IntPtr sid,out IntPtr text);[DllImport("kernel32.dll")]public static extern IntPtr LocalFree(IntPtr h);}' -PassThru|Out-Null;$sidPtr=[IntPtr]::Zero;$rc=[Phone02SidApi]::DeriveAppContainerSidFromAppContainerName($package.PackageFamilyName,[ref]$sidPtr);if($rc -ne 0){throw "SID derivation failed $rc"};$textPtr=[IntPtr]::Zero;if(-not[Phone02SidApi]::ConvertSidToStringSid($sidPtr,[ref]$textPtr)){throw 'SID stringify failed'};$expectedSid=[Runtime.InteropServices.Marshal]::PtrToStringUni($textPtr);[Phone02SidApi]::LocalFree($textPtr)|Out-Null;[Phone02SidApi]::LocalFree($sidPtr)|Out-Null;$script:expectedSid=$expectedSid;Write-Host "EXPECTED_APPCONTAINER_SID=$expectedSid"
 Round 30 'confirm package AUMID and activation manager';$script:aumid="$($package.PackageFamilyName)!App";Write-Host "AUMID=$script:aumid"
 Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
[ComImport,Guid("2e941141-7f97-4756-ba1d-9decde894a3d"),InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IApplicationActivationManager{[PreserveSig]int ActivateApplication([MarshalAs(UnmanagedType.LPWStr)]string appUserModelId,[MarshalAs(UnmanagedType.LPWStr)]string arguments,uint options,out uint processId);}
[ComImport,Guid("45BA127D-10A8-46EA-8AB7-56EA9078943C"),ClassInterface(ClassInterfaceType.None)]public class ApplicationActivationManager{}
public static class PackageActivator{[DllImport("ole32.dll")]private static extern int CoInitializeEx(IntPtr p,uint flags);[DllImport("ole32.dll")]private static extern void CoUninitialize();public static uint Activate(string id,string args){int hr=CoInitializeEx(IntPtr.Zero,2);if(hr<0&&hr!=unchecked((int)0x80010106))Marshal.ThrowExceptionForHR(hr);IApplicationActivationManager m=null;try{m=(IApplicationActivationManager)new ApplicationActivationManager();uint pid;hr=m.ActivateApplication(id,args,0,out pid);if(hr<0)Marshal.ThrowExceptionForHR(hr);return pid;}finally{if(m!=null&&Marshal.IsComObject(m))Marshal.ReleaseComObject(m);if(hr>=0)CoUninitialize();}}}
'@
 Round 31 'create RPC endpoint with user and AppContainer SID ACL';$server=Start-RpcServer 'legitimate'
 Round 32 'confirm ncalrpc endpoint ready';if((Get-Content $server.Out -Raw) -notmatch 'RPC_SERVER_READY=TRUE'){throw 'RPC server readiness marker missing'}
 Round 33 'activate real MSIX AppContainer client';$legitPid=Activate-Client 'rpc' $server.Endpoint
 Round 34 'verify packaged client RPC binding';$legitClient=Wait-ClientLog $legitPid 'RPC_CLIENT_CALL_STATUS=';if($legitClient.Content -notmatch 'RPC_CLIENT_BINDING=PASS'){throw 'RPC client binding failed'}
 Round 35 'observe real client PID through RPC runtime';$legitServer=Finish-RpcServer $server;if($legitServer.Content -notmatch "RPC_CLIENT_PID=$legitPid(\s|$)"){throw 'RPC observed PID mismatch'}
 Round 36 'verify AppContainer flag in process and impersonation tokens';if($legitServer.Content -notmatch 'TOKEN_IS_APPCONTAINER=1 THREAD_TOKEN_IS_APPCONTAINER=1'){throw 'AppContainer token not observed'}
 Round 37 'verify Package Family and Full Name from process handle';if($legitServer.Content -notmatch [regex]::Escape("OBSERVED_PACKAGE_FAMILY=$($package.PackageFamilyName)") -or $legitServer.Content -notmatch [regex]::Escape("OBSERVED_PACKAGE_FULL_NAME=$($package.PackageFullName)")){throw 'Package identity mismatch'}
 Round 38 'verify PID/process/token correlation, session and user';if($legitServer.Content -notmatch 'PID_PROCESS_TOKEN_CORRELATION=PASS' -or $legitServer.Content -notmatch 'PROCESS_PATH_MATCH=PASS' -or $legitServer.Content -notmatch 'SESSION_MATCH=PASS' -or $legitServer.Content -notmatch 'USER_MATCH=PASS'){throw 'Token/process correlation failed'}
 Round 39 'require nonce-bound legitimate ALLOW';if($legitServer.Content -notmatch 'NONCE_MATCH=PASS NONCE_PID_MATCH=PASS NONCE_ONE_TIME=PASS' -or $legitServer.Content -notmatch 'AUTHORIZATION_RESULT=ALLOW' -or $legitClient.Content -notmatch 'RPC_CLIENT_AUTHORIZATION=ALLOW'){throw 'Legitimate RPC authorization failed'};$flags.Legitimate=$true;$oldNonce=[regex]::Match($legitClient.Content,'RPC_SESSION_NONCE=([0-9a-f]{32})').Groups[1].Value;if(-not $oldNonce){throw 'No session nonce logged'}
 Round 40 'same-user common process presents forged but matching claims';$common=Invoke-CommonCase 'same-user-common' 'common';if($common.Server -notmatch 'TOKEN_IS_APPCONTAINER=0' -or $common.Server -notmatch 'CLIENT_CLAIMS_MATCH_OBSERVED=TRUE' -or $common.Server -notmatch 'AUTHORIZATION_RESULT=REJECT'){throw 'Same-user process was not rejected'};$flags.CommonReject=$true
 Round 41 'prove same-user is not package identity';if($common.Server -notmatch 'PACKAGE_IDENTITY_MATCH=FAIL' -or $common.Server -notmatch 'PID_PROCESS_TOKEN_CORRELATION=PASS'){throw 'Adversary identity evidence incomplete'}
 Round 42 'spoof PID and AppContainer declaration';$p1=Invoke-CommonCase 'spoof-pid' 'spoof-pid';$p2=Invoke-CommonCase 'spoof-app' 'spoof-app';if($p1.Server -notmatch 'AUTHORIZATION_RESULT=REJECT' -or $p2.Server -notmatch 'AUTHORIZATION_RESULT=REJECT'){throw 'PID/AppContainer spoof accepted'};$flags.SpoofPid=$true;$flags.SpoofApp=$true
 Round 43 'spoof AppContainer SID';$s1=Invoke-CommonCase 'spoof-sid' 'spoof-sid';if($s1.Server -notmatch 'CLIENT_CLAIMS_MATCH_OBSERVED=FALSE' -or $s1.Server -notmatch 'AUTHORIZATION_RESULT=REJECT'){throw 'SID spoof accepted'};$flags.SpoofSid=$true
 Round 44 'spoof Package Family and Full Name';$f1=Invoke-CommonCase 'spoof-family' 'spoof-family';$f2=Invoke-CommonCase 'spoof-full' 'spoof-full';if($f1.Server -notmatch 'AUTHORIZATION_RESULT=REJECT' -or $f2.Server -notmatch 'AUTHORIZATION_RESULT=REJECT'){throw 'Package identity spoof accepted'};$flags.SpoofFamily=$true;$flags.SpoofFull=$true
 Round 45 'spoof all claims from actual packaged client';$sp=Invoke-PackagedCase 'packaged-spoof-all' 'spoof-all';if($sp.Server -notmatch 'TOKEN_IS_APPCONTAINER=1' -or $sp.Server -notmatch 'CLIENT_CLAIMS_MATCH_OBSERVED=FALSE' -or $sp.Server -notmatch 'AUTHORIZATION_RESULT=REJECT'){throw 'Packaged spoof claims were not rejected'};$flags.SpoofPackage=$true
 Round 46 'activate two independent package instances concurrently';$sA=Start-RpcServer 'instance-a';$sB=Start-RpcServer 'instance-b';$pidA=Activate-Client 'rpc-delay' $sA.Endpoint;$pidB=Activate-Client 'rpc-delay' $sB.Endpoint;$cA=Wait-ClientLog $pidA 'RPC_CLIENT_CALL_STATUS=';$cB=Wait-ClientLog $pidB 'RPC_CLIENT_CALL_STATUS=';$rA=Finish-RpcServer $sA;$rB=Finish-RpcServer $sB;if($pidA -eq $pidB -or $rA.Content -notmatch "RPC_CLIENT_PID=$pidA(\s|$)" -or $rB.Content -notmatch "RPC_CLIENT_PID=$pidB(\s|$)" -or $rA.Content -notmatch 'AUTHORIZATION_RESULT=ALLOW' -or $rB.Content -notmatch 'AUTHORIZATION_RESULT=ALLOW'){throw 'Two package instances not independently authorized'};Write-Host "INSTANCE_PID_1=$pidA INSTANCE_PID_2=$pidB PACKAGE_IDENTITY_SHARED=TRUE";$flags.TwoInstances=$true
 Round 47 'reconnect after process exit using new PID and nonce';$reconnect=Invoke-PackagedCase 'reconnect' 'rpc';if($reconnect.Pid -eq $legitPid -or $reconnect.Server -notmatch 'AUTHORIZATION_RESULT=ALLOW' -or $reconnect.Server -notmatch 'NONCE_MATCH=PASS'){throw 'Reconnect authorization failed'};$flags.Reconnect=$true
 Round 48 'replay old session nonce against new one-time server session';$replayServer=Start-RpcServer 'replay-target';$beginPid=Activate-Client 'begin-only' $replayServer.Endpoint;$begin=Wait-ClientLog $beginPid 'RPC_CLIENT_BEGIN_ONLY=PASS';$fresh=[regex]::Match($begin.Content,'RPC_SESSION_NONCE=([0-9a-f]{32})').Groups[1].Value;if(-not $fresh -or $fresh -eq $oldNonce){throw 'Fresh nonce absent or reused'};$out=Join-Path $root 'replay-client.log';$err=Join-Path $root 'replay-client.err';$ra=@('--rpc-replay',$package.PackageFamilyName,$expectedSid,$package.PackageFullName,$replayServer.Endpoint,$oldNonce)|ForEach-Object {'"'+($_ -replace '"','\"')+'"'};$rp=Start-Process (Join-Path $root 'common.exe') -ArgumentList ($ra -join ' ') -PassThru -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError $err;if(-not $rp.WaitForExit(20000)){try{$rp.Kill()}catch{};throw 'Replay client timeout'};$rr=Finish-RpcServer $replayServer;Write-Host (Get-Content $out -Raw -ErrorAction SilentlyContinue);if($rr.Content -notmatch 'NONCE_MATCH=FAIL' -or $rr.Content -notmatch 'NONCE_PID_MATCH=FAIL' -or $rr.Content -notmatch 'AUTHORIZATION_RESULT=REJECT'){throw 'Prior-session replay was not rejected'};$flags.Replay=$true
 Round 49 'cleanup package, test certificate, processes and verify baseline'
}finally{
 foreach($p in $rpcServers){try{if(-not $p.HasExited){Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue}}catch{}}
 $alive=@($rpcServers|Where-Object{try{-not $_.HasExited}catch{$false}})
 if(-not $package){$package=Get-AppxPackage -Name 'TokenSaver.Phone02.IdentityPoA' -ErrorAction SilentlyContinue}
 if($package){Remove-AppxPackage -Package $package.PackageFullName -ErrorAction SilentlyContinue}
 $remaining=Get-AppxPackage -Name 'TokenSaver.Phone02.IdentityPoA' -ErrorAction SilentlyContinue
 Write-Host "CLEANUP_PACKAGE_REMAINS=$([bool]$remaining) RPC_SUPERVISORS_REMAINS=$($alive.Count)"
 if($cert){foreach($store in @('Cert:\CurrentUser\My','Cert:\CurrentUser\Root','Cert:\CurrentUser\TrustedPeople','Cert:\LocalMachine\TrustedPeople')){Remove-Item "$store\$($cert.Thumbprint)" -ErrorAction SilentlyContinue};$left=@(Get-ChildItem Cert:\CurrentUser\My,Cert:\CurrentUser\Root,Cert:\CurrentUser\TrustedPeople,Cert:\LocalMachine\TrustedPeople -ErrorAction SilentlyContinue|Where-Object Thumbprint -eq $cert.Thumbprint);Write-Host "CLEANUP_CERT_REMAINS=$($left.Count -gt 0)"}
 $changes=@(git diff --name-only "$baseline..HEAD");$unexpected=@($changes|Where-Object{$_ -notlike 'scripts/experimental/*' -and $_ -ne '.github/workflows/phone-02-windows-identity-poa.yml'});Write-Host "FINAL_BASELINE_DIFF=$($changes -join ';')";Write-Host "PRODUCTION_FILES_CHANGED=$($unexpected.Count -gt 0)"
 $flags.Cleanup=($alive.Count -eq 0)-and(-not $remaining);$flags.ProductionUntouched=($unexpected.Count -eq 0)
 Write-Host "CLEANUP_COMPLETE=$($flags.Cleanup)"
 try{Stop-Transcript|Out-Null}catch{}
}
Round 50 'final classification and evidence summary'
$failed=@($flags.GetEnumerator()|Where-Object{-not $_.Value})
if($failed.Count -eq 0){$finalClassification='PASS'}elseif($flags.Legitimate -or $flags.CommonReject){$finalClassification='NO-GO'}else{$finalClassification='BLOCKED'}
Write-Host "P2.34_CLASSIFICATION=$finalClassification"
foreach($k in $flags.Keys){Write-Host "CONTROL_$($k.ToUpper())=$($flags[$k])"}
Write-Host "FINAL_HEAD=$((git rev-parse HEAD).Trim()) BASELINE=$baseline BRANCH=$branch"
if($finalClassification -ne 'PASS'){throw "P2.34 incomplete: $finalClassification; failed controls=$($failed.Name -join ',')"}
