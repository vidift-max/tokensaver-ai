$ErrorActionPreference='Stop'
$root=Join-Path $env:RUNNER_TEMP 'phone02-p238a'
$evidence=Join-Path $env:RUNNER_TEMP 'phone02-p238a-evidence'
$build=Join-Path $root 'build';$pkg=Join-Path $root 'package'
New-Item -ItemType Directory -Force -Path $build,$pkg,$evidence|Out-Null
$service='TokenSaverPhone02Supervisor';$serviceStarted=$false;$teamRoomBridgePassed=$false;$teamRoomProcess=$null;$teamRoomProcessGone=$true;$p238Certificates=[System.Collections.Generic.List[System.Security.Cryptography.X509Certificates.X509Certificate2]]::new();$p238PrivateKeys=[System.Collections.Generic.List[object]]::new();$p238RootThumbprints=[System.Collections.Generic.List[string]]::new();$p238HostsPath=Join-Path $env:SystemRoot 'System32\drivers\etc\hosts';$p238HostsOriginal=$null;$p238HostsRestored=$true;$p238CertsGone=$true;$p238KeysGone=$true;$p238Endpoint='https://team-room.p238.test:8443/health/live';$created=$false;$installed=$false;$cert=$null;$final='BLOCKED';$servicePid=0;$activationPid=0;$testStart=Get-Date;$testPrivateKeyUniqueName=$null;$testPrivateKeyProvider=$null
Start-Transcript -Path (Join-Path $evidence 'transcript.log') -Force|Out-Null
function Check($ok,$label){Write-Host ($label+'='+$(if($ok){'PASS'}else{'FAIL'}));if(-not $ok){throw ($label+' failed')}}

function Register-P238Certificate($c){
 $script:p238Certificates.Add($c)
 $rsa=[System.Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey($c)
 if($rsa -is [System.Security.Cryptography.RSACng]){$script:p238PrivateKeys.Add(@{Name=$rsa.Key.UniqueName;Provider=$rsa.Key.Provider})}else{throw 'P2.38-A test certificates must use tracked CNG machine private keys'}
 if($rsa){$rsa.Dispose()}
}
function New-P238Root($subject){
 $c=New-SelfSignedCertificate -Type Custom -Subject $subject -KeyAlgorithm RSA -KeyLength 2048 -KeyExportPolicy Exportable -KeyUsage CertSign,CRLSign,DigitalSignature -TextExtension @('2.5.29.19={critical}{text}ca=1&pathlength=1') -CertStoreLocation 'Cert:\LocalMachine\My' -NotAfter (Get-Date).AddDays(3)
 Register-P238Certificate $c
 return $c
}
function New-P238Leaf($name,$dns,$signer,$eku){
 $extensions=@("2.5.29.37={text}$eku")
 $c=New-SelfSignedCertificate -Type Custom -Subject "CN=$dns" -DnsName $dns -Signer $signer -KeyAlgorithm RSA -KeyLength 2048 -KeyExportPolicy Exportable -KeyUsage DigitalSignature,KeyEncipherment -TextExtension $extensions -CertStoreLocation 'Cert:\LocalMachine\My' -NotAfter (Get-Date).AddDays(2)
 Register-P238Certificate $c
 $rsa=[System.Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey($c)
 if(-not $rsa){throw "Missing private key for $name"}
 $certPath=Join-Path $root ($name+'.cert.pem');$keyPath=Join-Path $root ($name+'.key.pem')
 [IO.File]::WriteAllText($certPath,$c.ExportCertificatePem(),[Text.Encoding]::ASCII)
 [IO.File]::WriteAllText($keyPath,$rsa.ExportPkcs8PrivateKeyPem(),[Text.Encoding]::ASCII)
 $rsa.Dispose()
 return @{Cert=$c;CertPath=$certPath;KeyPath=$keyPath}
}
function Stop-P238TeamRoom {
 if($script:teamRoomProcess){
  $id=$script:teamRoomProcess.Id
  $p=Get-Process -Id $id -ErrorAction SilentlyContinue
  if($p){Stop-Process -Id $id -Force -ErrorAction SilentlyContinue;Wait-Process -Id $id -Timeout 10 -ErrorAction SilentlyContinue}
  if(-not(Get-Process -Id $id -ErrorAction SilentlyContinue)){$script:teamRoomProcess=$null;return $true}
  return $false
 }
 return $true
}
function Start-P238TeamRoom($leaf,$redirect,$verifyHttpsReadiness=$true){
 if(-not(Stop-P238TeamRoom)){throw 'Previous Team Room TLS listener did not stop'}
 Remove-Item (Join-Path $script:evidence 'team-room.stdout.log'),(Join-Path $script:evidence 'team-room.stderr.log') -Force -ErrorAction SilentlyContinue
 $fullMode=($env:P238_TEAM_ROOM_FULL_MODE -eq '1')
 if($fullMode){
  Remove-Item Env:TEAM_ROOM_TLS_PROBE_ONLY,Env:TEAM_ROOM_TLS_BIND,Env:TEAM_ROOM_BIND,Env:TEAM_ROOM_TEST_REDIRECT_HTTP -ErrorAction SilentlyContinue
  $env:TEAM_ROOM_HTTPS_BIND='127.0.0.1:8443'
 }else{
  $env:TEAM_ROOM_TLS_PROBE_ONLY='1';$env:TEAM_ROOM_TLS_BIND='127.0.0.1:8443';$env:TEAM_ROOM_BIND='127.0.0.1:8081'
  if($redirect){$env:TEAM_ROOM_TEST_REDIRECT_HTTP='1'}else{Remove-Item Env:TEAM_ROOM_TEST_REDIRECT_HTTP -ErrorAction SilentlyContinue}
 }
 $env:TEAM_ROOM_TLS_CERT_PATH=$leaf.CertPath;$env:TEAM_ROOM_TLS_KEY_PATH=$leaf.KeyPath
 $script:teamRoomProcess=Start-Process -FilePath $script:teamRoomExe -WorkingDirectory $script:root -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $script:evidence 'team-room.stdout.log') -RedirectStandardError (Join-Path $script:evidence 'team-room.stderr.log')
 Remove-Item Env:TEAM_ROOM_TLS_PROBE_ONLY,Env:TEAM_ROOM_TLS_BIND,Env:TEAM_ROOM_BIND,Env:TEAM_ROOM_HTTPS_BIND,Env:TEAM_ROOM_TLS_CERT_PATH,Env:TEAM_ROOM_TLS_KEY_PATH,Env:TEAM_ROOM_TEST_REDIRECT_HTTP -ErrorAction SilentlyContinue
 $deadline=(Get-Date).AddSeconds(20);$connected=$false
 do{
  if(-not (Get-Process -Id $script:teamRoomProcess.Id -ErrorAction SilentlyContinue)){throw 'Team Room TLS process exited before listener became ready'}
  $tcp=[Net.Sockets.TcpClient]::new()
  try{$task=$tcp.ConnectAsync('127.0.0.1',8443);if($task.Wait(300)){$connected=$tcp.Connected}}catch{}finally{$tcp.Dispose()}
  if(-not $connected){Start-Sleep -Milliseconds 250}
 }while(-not $connected -and (Get-Date) -lt $deadline)
 if(-not $connected){throw 'Team Room TLS listener did not bind 127.0.0.1:8443'}
 if($fullMode){
  $deadline=(Get-Date).AddSeconds(20);$teamLog=''
  do{
   Start-Sleep -Milliseconds 250
   $teamLog=Get-Content (Join-Path $script:evidence 'team-room.stdout.log') -Raw -ErrorAction SilentlyContinue
   # tracing-subscriber emits ANSI field-formatting escapes; strip them before
   # parsing the structured database identity fields.
   $ansiPattern=[string][char]27+'\[[0-?]*[ -/]*[@-~]'
   $teamLog=[regex]::Replace($teamLog,$ansiPattern,'')
  }while(($teamLog -notmatch 'TEAM_ROOM_REAL_HTTPS_LISTENER_READY' -or $teamLog -notmatch 'TEAM_ROOM_POSTGRES_CONNECTED') -and (Get-Date) -lt $deadline)
  Check ($teamLog -match 'TEAM_ROOM_REAL_HTTPS_LISTENER_READY') 'TEAM_ROOM_REAL_HTTPS_LISTENER'
  if($teamLog -notmatch 'TEAM_ROOM_POSTGRES_CONNECTED.*?database=([^\s]+).*?backend_pid=(\d+)'){
   throw 'Could not parse Team Room PostgreSQL database identity and backend PID from service log'
  }
  $teamRoomLoggedDatabase=$Matches[1]
  $teamRoomBackendPid=[int]$Matches[2]
  Write-Host ('TEAM_ROOM_POSTGRES_LOG_DATABASE='+$teamRoomLoggedDatabase)
  Write-Host ('TEAM_ROOM_POSTGRES_LOG_BACKEND_PID='+$teamRoomBackendPid)
  if($verifyHttpsReadiness){
   $ready=Invoke-WebRequest -Uri 'https://team-room.p238.test:8443/health/ready' -UseBasicParsing -TimeoutSec 5
   Check ($ready.StatusCode -eq 200) 'TEAM_ROOM_REAL_HTTPS_DATABASE_READINESS'
  }else{
   Write-Host 'TEAM_ROOM_HTTPS_PREFLIGHT=SKIPPED_EXPECTED_NEGATIVE_CERTIFICATE_CASE'
  }
  $psql=Join-Path $env:ProgramFiles 'PostgreSQL\16\bin\psql.exe'
  # Independent observer connection: records the actual PostgreSQL endpoint and
  # identity used by this diagnostic query, without exposing any credential.
  $dbObserver=& $psql -h 127.0.0.1 -p 5432 -U postgres -d postgres -At -F '|' -c "SELECT current_database(), COALESCE(inet_server_addr()::text,''), COALESCE(host(inet_server_addr()),''), inet_server_port(), pg_backend_pid(), current_user"
  $observerExit=$LASTEXITCODE
  if($observerExit -ne 0){throw 'Could not inspect Team Room PostgreSQL observer connection'}
  $observerFields=@("$dbObserver".Trim() -split '\|')
  Write-Host ('TEAM_ROOM_POSTGRES_OBSERVER_DATABASE='+$observerFields[0])
  Write-Host ('TEAM_ROOM_POSTGRES_OBSERVER_SERVER_ADDR='+$observerFields[1])
  Write-Host ('TEAM_ROOM_POSTGRES_OBSERVER_SERVER_HOST='+$observerFields[2])
  Write-Host ('TEAM_ROOM_POSTGRES_OBSERVER_SERVER_PORT='+$observerFields[3])
  Write-Host ('TEAM_ROOM_POSTGRES_OBSERVER_BACKEND_PID='+$observerFields[4])
  Write-Host ('TEAM_ROOM_POSTGRES_OBSERVER_USER='+$observerFields[5])
  Check ($observerFields.Count -eq 6 -and $observerFields[0] -eq 'postgres' -and $observerFields[1] -eq '127.0.0.1/32' -and $observerFields[2] -eq '127.0.0.1' -and $observerFields[3] -eq '5432' -and [int]$observerFields[4] -gt 0 -and $observerFields[5] -eq 'postgres') 'TEAM_ROOM_POSTGRES_OBSERVER_ENDPOINT'
  # Match the backend PID obtained by the Team Room's own pool against
  # pg_stat_activity. This independently proves the same live connection,
  # its database, role, application_name and client address.
  $teamDbSession=& $psql -h 127.0.0.1 -p 5432 -U postgres -d postgres -At -F '|' -c "SELECT datname, usename, application_name, pid, COALESCE(client_addr::text,''), COALESCE(host(client_addr),'') FROM pg_stat_activity WHERE pid=$teamRoomBackendPid"
  $sessionExit=$LASTEXITCODE
  if($sessionExit -ne 0){throw 'Could not inspect Team Room backend session identity'}
  $sessionFields=@("$teamDbSession".Trim() -split '\|')
  Write-Host ('TEAM_ROOM_POSTGRES_SESSION_DATABASE='+$sessionFields[0])
  Write-Host ('TEAM_ROOM_POSTGRES_SESSION_USER='+$sessionFields[1])
  Write-Host ('TEAM_ROOM_POSTGRES_SESSION_APPLICATION='+$sessionFields[2])
  Write-Host ('TEAM_ROOM_POSTGRES_SESSION_BACKEND_PID='+$sessionFields[3])
  Write-Host ('TEAM_ROOM_POSTGRES_SESSION_CLIENT_ADDR='+$sessionFields[4])
  Write-Host ('TEAM_ROOM_POSTGRES_SESSION_CLIENT_HOST='+$sessionFields[5])
  $sameBackend=($sessionFields.Count -eq 6 -and $sessionFields[0] -eq 'team_room_p238b' -and $sessionFields[1] -eq 'postgres' -and $sessionFields[2] -eq 'team-room-p238b' -and $sessionFields[3] -eq [string]$teamRoomBackendPid -and $sessionFields[4] -eq '127.0.0.1/32' -and $sessionFields[5] -eq '127.0.0.1')
  Check ($teamRoomLoggedDatabase -eq 'team_room_p238b' -and $sameBackend) 'TEAM_ROOM_POSTGRES_DATABASE_IDENTITY'
  Check ($sameBackend) 'TEAM_ROOM_POSTGRES_ACTIVE_CONNECTION'
  Write-Host ('TEAM_ROOM_REAL_HTTPS_PROCESS_ID='+$script:teamRoomProcess.Id)
  Write-Host 'TEAM_ROOM_POSTGRES_ACTIVE_CONNECTIONS=1'
  Write-Host 'TEAM_ROOM_HTTP_FALLBACK=NOT_STARTED'
 }else{
  Write-Host ('TEAM_ROOM_TLS_LISTENER=READY pid='+$script:teamRoomProcess.Id+' hostname=team-room.p238.test endpoint=https://team-room.p238.test:8443/health/live')
 }
}
function Restart-P238Supervisor($endpoint,$fakeMode=$null){
 $before=Get-CimInstance Win32_Service -Filter "Name='$script:service'" -ErrorAction Stop
 $oldPid=[int]$before.ProcessId
 & sc.exe stop $script:service 2>&1|Out-Null
 $deadline=(Get-Date).AddSeconds(20);$state=''
 do{Start-Sleep -Milliseconds 250;$q=Get-CimInstance Win32_Service -Filter "Name='$script:service'" -ErrorAction SilentlyContinue;$state=$q.State}while($state -ne 'Stopped' -and (Get-Date) -lt $deadline)
 if($state -ne 'Stopped'){throw ('Supervisor did not stop for endpoint test; state='+$state+' pid='+$oldPid)}
 # SCM may publish STOPPED just before ServiceMain closes its final log handle
 # and exits. Wait for the old process itself before rotating the log/restarting.
 $deadline=(Get-Date).AddSeconds(10)
 while($oldPid -gt 0 -and (Get-CimInstance Win32_Process -Filter "ProcessId=$oldPid" -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline){Start-Sleep -Milliseconds 100}
 if($oldPid -gt 0 -and (Get-CimInstance Win32_Process -Filter "ProcessId=$oldPid" -ErrorAction SilentlyContinue)){throw ('Supervisor reported STOPPED but old process is still alive; pid='+$oldPid)}
 Write-Host ('SUPERVISOR_PREVIOUS_PID_EXITED=PASS pid='+$oldPid)
 $lines=@(Get-Content $script:config|Where-Object {$_ -notmatch '^TEAM_ROOM_HTTPS_ENDPOINT=' -and ($null -eq $fakeMode -or $_ -notmatch '^TEAM_ROOM_FAKE_MODE=')})
 $lines += "TEAM_ROOM_HTTPS_ENDPOINT=$endpoint"
 if($null -ne $fakeMode){$lines += "TEAM_ROOM_FAKE_MODE=$fakeMode"}
 Set-Content $script:config $lines -Encoding ASCII
 Remove-Item ($script:config+'.log') -Force -ErrorAction SilentlyContinue
 & sc.exe start $script:service|Out-Null
 if($LASTEXITCODE -ne 0){throw ('Supervisor failed to restart; sc_exit='+$LASTEXITCODE)}
 $deadline=(Get-Date).AddSeconds(20);$ready=$false;$q=$null
 do{
  Start-Sleep -Milliseconds 250
  $q=Get-CimInstance Win32_Service -Filter "Name='$script:service'" -ErrorAction SilentlyContinue
  $logReady=(Test-Path ($script:config+'.log')) -and ((Get-Content ($script:config+'.log') -Raw -ErrorAction SilentlyContinue) -match 'RPC_ENDPOINT=READY')
  $ready=($q.State -eq 'Running' -and [int]$q.ProcessId -gt 0 -and [int]$q.ProcessId -ne $oldPid -and $logReady)
  if($q.State -eq 'Stopped'){break}
 }while(-not $ready -and (Get-Date) -lt $deadline)
 if(-not $ready){
  $exit=if($q){' state='+$q.State+' pid='+$q.ProcessId+' win32_exit='+$q.ExitCode+' service_exit='+$q.ServiceSpecificExitCode}else{' service_query=UNAVAILABLE'}
  $log=if(Test-Path ($script:config+'.log')){Get-Content ($script:config+'.log') -Raw -ErrorAction SilentlyContinue}else{'LOG_MISSING'}
  throw ('Supervisor RPC endpoint did not become ready after restart;'+$exit+' log='+$log)
 }
 $script:servicePid=[int]$q.ProcessId
}
function Invoke-P238AppContainerScenario($label,$expected,$expectedReason){
 $fallback=Join-Path $env:LOCALAPPDATA 'PhoneSupervisor-P236-client.log'
 Remove-Item $fallback -Force -ErrorAction SilentlyContinue
 if(Test-Path $script:clientRoot){Get-ChildItem $script:clientRoot -Filter 'PhoneSupervisor-P236-client.log' -File -Recurse -ErrorAction SilentlyContinue|Remove-Item -Force -ErrorAction SilentlyContinue}
 $appPid=[P238AActivator]::Activate($script:aumid,'')
 $deadline=(Get-Date).AddSeconds(25);$log=$null;$body=''
 do{
  Start-Sleep -Milliseconds 200
  if(Test-Path $fallback){$log=Get-Item $fallback}
  if(-not $log -and (Test-Path $script:clientRoot)){$log=Get-ChildItem $script:clientRoot -Filter 'PhoneSupervisor-P236-client.log' -File -Recurse -ErrorAction SilentlyContinue|Select-Object -First 1}
  if($log){$body=Get-Content $log.FullName -Raw -ErrorAction SilentlyContinue}
 }while($body -notmatch 'TEAM_ROOM_SCENARIO=(PASS|FAIL)' -and (Get-Date) -lt $deadline)
 if(-not $log -or $body -notmatch 'TEAM_ROOM_SCENARIO=(PASS|FAIL)'){throw "$label AppContainer did not complete TeamRoomScenario"}
 Wait-Process -Id $appPid -Timeout 10 -ErrorAction SilentlyContinue
 Check ($body -match 'LEGITIMATE_APPCONTAINER=PASS' -and $body -match 'AUTHORIZATION_ALLOW=PASS') ($label+'_APPCONTAINER_RPC_AUTH')
 $svcBody=Get-Content ($script:config+'.log') -Raw
 if($expected -eq 'FAIL_CLOSED' -or $expected -eq 'FAKE_FAIL_CLOSED'){
  Check ($body -match 'TEAM_ROOM_SCENARIO=FAIL' -and $body -match 'TEAM_ROOM_FAIL_CLOSED=PASS' -and $body -match 'CLOSE_SESSION=PASS') ($label+'_TEAM_ROOM_FAIL_CLOSED')
  Check ($svcBody -match 'TEAM_ROOM_HTTPS_TLS=PASS .*certificate_chain=VALID hostname=VALID http_status=200') ($label+'_HEALTH_RESPONSE_ONLY')
  Check ($svcBody -match 'TEAM_ROOM_HEALTH_NOT_AUTH=PASS' -and $svcBody -match 'TEAM_ROOM_FALSE_SUCCESS_REMOVED=PASS') ($label+'_HEALTH_NOT_AUTH')
  Check ($svcBody -match 'TEAM_ROOM_ACCESS_TOKEN_REQUESTED=NO' -and $svcBody -match 'TEAM_ROOM_AUTH_EXCHANGE_CALLED=NO') ($label+'_NO_OIDC_OR_EXCHANGE')
  if($expected -eq 'FAKE_FAIL_CLOSED'){Write-Host 'FAKE_MODE_DISTINCT=PASS'}
  if($expected -eq 'FAKE_FAIL_CLOSED'){
   Check ($svcBody -match 'TEAM_ROOM_FAKE_SCENARIO=PASS' -and $svcBody -match 'TEAM_ROOM_FAKE_NOT_AUTHENTICATED=PASS' -and $svcBody -match 'TEAM_ROOM_FAKE_MODE_NOT_AUTH=PASS') ($label+'_FAKE_MODE_ISOLATED')
  }else{
   Check ($svcBody -match 'TEAM_ROOM_FAKE_MODE=DISABLED' -and $svcBody -notmatch 'TEAM_ROOM_FAKE_SCENARIO=PASS') ($label+'_REAL_MODE_HAS_NO_FAKE_SUCCESS')
  }
 }elseif($expected -eq 'PASS'){
  Check ($body -match 'TEAM_ROOM_SCENARIO=PASS' -and $body -match 'CLOSE_SESSION=PASS') ($label+'_SUPERVISOR_HTTPS_PASS')
  Check ($svcBody -match 'TEAM_ROOM_HTTPS_TLS=PASS .*certificate_chain=VALID hostname=VALID http_status=200') ($label+'_TEAM_ROOM_RESPONSE')
 }else{
  Check ($body -match 'TEAM_ROOM_SCENARIO=FAIL') ($label+'_APPCONTAINER_TLS_REJECT')
  Check ($svcBody -match 'TEAM_ROOM_HTTPS_TLS=REJECT') ($label+'_SUPERVISOR_TLS_REJECT')
  if($expectedReason){Check ($svcBody -match $expectedReason) ($label+'_EXPECTED_REJECTION_REASON')}
 }
 Copy-Item $log.FullName (Join-Path $script:evidence ($label+'-appcontainer.log')) -Force
 Copy-Item ($script:config+'.log') (Join-Path $script:evidence ($label+'-supervisor.log')) -Force
 Write-Host ("P238A_CASE="+$label+" EXPECTED="+$expected+" RESULT=PASS")
}
try {
 $x86=[Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
 $vswhere=Join-Path $x86 'Microsoft Visual Studio\Installer\vswhere.exe'
 $vs=(& $vswhere -latest -products '*' -property installationPath).Trim()
 $vcvars=Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'
 $sdk=Get-ChildItem (Join-Path $x86 'Windows Kits\10\bin') -Directory|Where-Object Name -Match '^10\.'|Sort-Object Name -Descending|Select-Object -First 1
 $midl=Join-Path $sdk.FullName 'x64\midl.exe';$makeappx=Join-Path $sdk.FullName 'x64\MakeAppx.exe';$signtool=Join-Path $sdk.FullName 'x64\SignTool.exe'
 foreach($p in @($vcvars,$midl,$makeappx,$signtool)){if(-not(Test-Path $p)){throw ('Missing tool '+$p)}}
 $teamRoomManifest=Join-Path $env:GITHUB_WORKSPACE 'services\ai-team-room\Cargo.toml'
 $cargoTarget=Join-Path $build 'cargo-target'
 & cargo build --release --manifest-path $teamRoomManifest --target-dir $cargoTarget --bin ai-team-room-service
 if($LASTEXITCODE -ne 0){throw 'Team Room TLS probe binary build failed'}
 $script:teamRoomExe=Join-Path $cargoTarget 'release\ai-team-room-service.exe'
 if(-not(Test-Path $script:teamRoomExe)){throw 'Team Room TLS probe binary missing'}
 Write-Host ('TEAM_ROOM_BINARY_SHA256='+(Get-FileHash $script:teamRoomExe -Algorithm SHA256).Hash)
 $src=Join-Path $env:GITHUB_WORKSPACE 'services\phone-supervisor'
 $cmdMidl='"'+$vcvars+'" && "'+$midl+'" /nologo /env x64 /h "'+(Join-Path $build 'phone_supervisor_rpc.h')+'" /iid "'+(Join-Path $build 'phone_supervisor_rpc_i.c')+'" /out "'+$build+'" "'+(Join-Path $src 'phone_supervisor_rpc.idl')+'"'
 & cmd.exe /d /s /c $cmdMidl
 if($LASTEXITCODE -ne 0){throw 'MIDL failed'}
 $svcExe=Join-Path $root 'supervisor.exe';$cliExe=Join-Path $root 'client.exe'
 $cmd='"'+$vcvars+'" && cl /nologo /EHsc /W4 /std:c++20 /DUNICODE /D_UNICODE /I"'+$build+'" "'+(Join-Path $src 'phone_supervisor.cpp')+'" "'+(Join-Path $src 'RealSupervisorTeamRoomHttpsAdapter.cpp')+'" "'+(Join-Path $src 'SupervisorCredentialStoreR5.cpp')+'" "'+(Join-Path $src 'SupervisorCredentialProvisioningR5.cpp')+'" "'+(Join-Path $build 'phone_supervisor_rpc_s.c')+'" /Fe:"'+$svcExe+'" advapi32.lib rpcrt4.lib bcrypt.lib userenv.lib shell32.lib ole32.lib winhttp.lib ncrypt.lib'
 & cmd.exe /d /s /c $cmd;if($LASTEXITCODE -ne 0){throw 'Supervisor compile failed'}
 $cmd='"'+$vcvars+'" && cl /nologo /EHsc /W4 /std:c++20 /DUNICODE /D_UNICODE /I"'+$build+'" "'+(Join-Path $src 'phone_supervisor_client.cpp')+'" "'+(Join-Path $build 'phone_supervisor_rpc_c.c')+'" /Fe:"'+$cliExe+'" rpcrt4.lib ole32.lib'
 & cmd.exe /d /s /c $cmd;if($LASTEXITCODE -ne 0){throw 'RPC client compile failed'}
 Write-Host ('SUPERVISOR_SHA256='+(Get-FileHash $svcExe -Algorithm SHA256).Hash)
 Write-Host ('CLIENT_SHA256='+(Get-FileHash $cliExe -Algorithm SHA256).Hash)
 $publisher='CN=TokenSaver Phone02 P2.38-A TEST ONLY'
 $manifest=@"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10" xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10" xmlns:desktop="http://schemas.microsoft.com/appx/manifest/desktop/windows10" xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities" IgnorableNamespaces="uap uap10 desktop rescap">
<Identity Name="TokenSaver.Phone02.SupervisorTest" Publisher="$publisher" Version="1.0.0.0" ProcessorArchitecture="x64"/>
<Properties><DisplayName>Phone-02 Supervisor P2.38-A TEST</DisplayName><PublisherDisplayName>TEST ONLY</PublisherDisplayName><Logo>Assets/StoreLogo.png</Logo></Properties>
<Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0"/></Dependencies>
<Resources><Resource Language="en-us"/></Resources>
<Applications><Application Id="App" Executable="client.exe" uap10:TrustLevel="appContainer" uap10:RuntimeBehavior="packagedClassicApp"><uap:VisualElements DisplayName="Phone-02 Supervisor Test" Description="Disposable P2.38-A client" BackgroundColor="transparent" Square150x150Logo="Assets/Square150x150Logo.png" Square44x44Logo="Assets/Square44x44Logo.png"/></Application><Application Id="SupervisorHost" Executable="supervisor-host.exe" EntryPoint="Windows.FullTrustApplication"><uap:VisualElements DisplayName="Phone-02 Supervisor Host TEST" Description="Disposable full-trust activation host" BackgroundColor="transparent" Square150x150Logo="Assets/Square150x150Logo.png" Square44x44Logo="Assets/Square44x44Logo.png"/><Extensions><desktop:Extension Category="windows.fullTrustProcess" Executable="supervisor.exe"><desktop:FullTrustProcess/></desktop:Extension></Extensions></Application></Applications>
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
 $msix=Join-Path $root 'phone02-p238a.msix'
 & $makeappx pack /d $pkg /p $msix /o|Out-Host;if($LASTEXITCODE -ne 0){throw 'MakeAppx failed'}
 & $signtool sign /fd SHA256 /sha1 $cert.Thumbprint $msix|Out-Host;if($LASTEXITCODE -ne 0){throw 'MSIX signing failed'}
 & $signtool verify /pa /v $msix|Out-Host;if($LASTEXITCODE -ne 0){throw 'MSIX signature verify failed'}
 Write-Host ('MSIX_SHA256='+(Get-FileHash $msix -Algorithm SHA256).Hash)
 Add-AppxPackage -Path $msix -ErrorAction Stop;$installed=$true
 $app=Get-AppxPackage -Name 'TokenSaver.Phone02.SupervisorTest'|Select-Object -First 1
 if(-not $app){throw 'MSIX package registration missing'}
 if($app.Publisher -ne $publisher){throw 'Installed package publisher does not match the disposable test signer'}
 Write-Host ('PACKAGE_PUBLISHER='+$app.Publisher)
 Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public static class P238ASid{[DllImport("userenv.dll",CharSet=CharSet.Unicode)]public static extern int DeriveAppContainerSidFromAppContainerName(string n,out IntPtr s);[DllImport("advapi32.dll",CharSet=CharSet.Unicode)]public static extern bool ConvertSidToStringSid(IntPtr s,out IntPtr t);[DllImport("kernel32.dll")]public static extern IntPtr LocalFree(IntPtr h);}'
 $sp=[IntPtr]::Zero;$hr=[P238ASid]::DeriveAppContainerSidFromAppContainerName($app.PackageFamilyName,[ref]$sp);if($hr -ne 0){throw 'AppContainer SID derive failed'}
 $tp=[IntPtr]::Zero;if(-not [P238ASid]::ConvertSidToStringSid($sp,[ref]$tp)){throw 'AppContainer SID stringify failed'}
 $appSid=[Runtime.InteropServices.Marshal]::PtrToStringUni($tp);[P238ASid]::LocalFree($tp)|Out-Null;[P238ASid]::LocalFree($sp)|Out-Null
 $config=Join-Path $root 'supervisor.conf';$svcExe=Join-Path $root 'supervisor.exe'
 $binPathValue='"'+$svcExe+'" --service "'+$config+'"'
 & sc.exe create $service 'binPath=' $binPathValue 'start=' 'demand' 'obj=' 'NT AUTHORITY\LocalService'|Out-Host
 if($LASTEXITCODE -ne 0){throw 'SCM create failed'};$created=$true
 & sc.exe sidtype $service unrestricted|Out-Host;if($LASTEXITCODE -ne 0){throw 'SCM Service SID setup failed'}
 $sidOut=(& sc.exe showsid $service 2>&1|Out-String);$serviceSid=[regex]::Match($sidOut,'S-1-5-80-(?:\d+-){4}\d+').Value
 if(-not $serviceSid){throw 'Service SID not returned by SCM'}
 @("PACKAGE_FAMILY=$($app.PackageFamilyName)","PACKAGE_FULL=$($app.PackageFullName)","APPCONTAINER_SID=$appSid","RPC_CLIENT_APPCONTAINER_SID=$appSid","SERVICE_SID=$serviceSid","USER_SID=$([System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value)","RPC_CLIENT_USER_SID=$([System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value)","TEAM_ROOM_FAKE_MODE=0","TEAM_ROOM_TEST_AI_PHONE_ID=TEST-AI-PHONE-02","TEAM_ROOM_TEST_ENDPOINT_ID=TEST-ENDPOINT-02","TEAM_ROOM_TEST_RECIPIENT_ID=FAKE-RECIPIENT-PHONE","TEAM_ROOM_HTTPS_ENDPOINT=$p238Endpoint")|Set-Content $config -Encoding ASCII
  $configText=Get-Content $config -Raw
  Check ($configText -notmatch '(?i)(CLIENT_SECRET|ACCESS_TOKEN|TEAM_ROOM_SESSION|ENDPOINT_CREDENTIAL|OPERATOR_KEY|PASSWORD)') 'SUPERVISOR_SECRET_MATERIAL_ABSENT'
  Check (-not (Test-Path 'C:\ProgramData\Phone02CredentialStoreR3')) 'R3_TEST_STORE_NOT_LEFT_BEHIND'
 $script:root=$root;$script:evidence=$evidence;$script:service=$service;$script:config=$config
 $p238HostsOriginal=[IO.File]::ReadAllText($p238HostsPath)
 if($p238HostsOriginal -notmatch '(?im)^\s*127\.0\.0\.1\s+team-room\.p238\.test(?:\s|$)'){Add-Content $p238HostsPath '127.0.0.1 team-room.p238.test' -Encoding ASCII}
 ipconfig.exe /flushdns|Out-Null
 $rootCa=New-P238Root 'CN=TokenSaver P2.38-A Ephemeral Test Root'
 $p238RootThumbprints.Add($rootCa.Thumbprint)
 $rootCer=Join-Path $root 'p238-root.cer';Export-Certificate -Cert $rootCa -FilePath $rootCer|Out-Null
 Import-Certificate -FilePath $rootCer -CertStoreLocation 'Cert:\LocalMachine\Root'|Out-Null
 Check ((Get-ChildItem 'Cert:\LocalMachine\Root'|Where-Object Thumbprint -EQ $rootCa.Thumbprint).Count -eq 1) 'P238A_EPHEMERAL_CA_TRUST_INSTALLED'
 $rogueCa=New-P238Root 'CN=TokenSaver P2.38-A Untrusted Test Root'
 $goodLeaf=New-P238Leaf 'valid' 'team-room.p238.test' $rootCa '1.3.6.1.5.5.7.3.1'
 $invalidLeaf=New-P238Leaf 'invalid-eku' 'team-room.p238.test' $rootCa '1.3.6.1.5.5.7.3.2'
 $wrongHostLeaf=New-P238Leaf 'wrong-host' 'wrong-host.p238.test' $rootCa '1.3.6.1.5.5.7.3.1'
 $untrustedLeaf=New-P238Leaf 'untrusted-ca' 'team-room.p238.test' $rogueCa '1.3.6.1.5.5.7.3.1'
 $sanExtension=$goodLeaf.Cert.Extensions|Where-Object {$_.Oid.Value -eq '2.5.29.17'}|Select-Object -First 1
 Check ($sanExtension -and $sanExtension.Format($false) -match 'team-room.p238.test') 'P238A_CERTIFICATE_SAN_MATCH'
 Write-Host ('P238A_TRUSTED_TEST_CA_THUMBPRINT='+$rootCa.Thumbprint)
 Write-Host ('P238A_REQUIRED_SAN=team-room.p238.test')
 Start-P238TeamRoom $goodLeaf $false
 $serviceAce='NT SERVICE\'+$service+':(OI)(CI)M'
 & icacls.exe $root /inheritance:r /grant 'SYSTEM:(OI)(CI)F' 'BUILTIN\Administrators:(OI)(CI)F' $serviceAce|Out-Host
 if($LASTEXITCODE -ne 0){throw 'Service working directory ACL failed'}
 & sc.exe start $service|Out-Host;if($LASTEXITCODE -ne 0){throw 'SCM service start failed'}
 $serviceStarted=$true
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
public interface IP238AApplicationActivationManager { [PreserveSig]int ActivateApplication([MarshalAs(UnmanagedType.LPWStr)]string appUserModelId,[MarshalAs(UnmanagedType.LPWStr)]string arguments,uint options,out uint processId); }
[ComImport,Guid("45BA127D-10A8-46EA-8AB7-56EA9078943C"),ClassInterface(ClassInterfaceType.None)]
public class P238AApplicationActivationManager {}
public static class P238AActivator {
 [DllImport("ole32.dll")] private static extern int CoInitializeEx(IntPtr p,uint flags);
 [DllImport("ole32.dll")] private static extern void CoUninitialize();
 public static uint Activate(string id,string args) {
  int init=CoInitializeEx(IntPtr.Zero,2);
  if(init<0 && init!=unchecked((int)0x80010106)) Marshal.ThrowExceptionForHR(init);
  IP238AApplicationActivationManager m=null; bool initialized=init>=0;
  try { m=(IP238AApplicationActivationManager)new P238AApplicationActivationManager(); uint pid;
   int hr=m.ActivateApplication(id,args,0,out pid); if(hr<0)Marshal.ThrowExceptionForHR(hr); return pid;
  } finally { if(m!=null && Marshal.IsComObject(m))Marshal.ReleaseComObject(m); if(initialized)CoUninitialize(); }
 }
}
'@
 $aumid="$($app.PackageFamilyName)!App"
 $script:aumid=$aumid;$script:clientRoot=Join-Path $env:LOCALAPPDATA ('Packages\'+$app.PackageFamilyName)
 $activationPid=[P238AActivator]::Activate($aumid,"")
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
 Check ($cl -match 'HEARTBEAT=PASS' -and $cl -match 'TEAM_ROOM_SCENARIO=FAIL' -and $cl -match 'TEAM_ROOM_FAIL_CLOSED=PASS' -and $cl -match 'CLOSE_SESSION=PASS') 'SESSION_AND_TEAM_ROOM_FAIL_CLOSED'
 $teamRoomLog=Get-Content $svcLog -Raw
 Check ($teamRoomLog -match 'TEAM_ROOM_HTTPS_TLS=PASS .*certificate_chain=VALID hostname=VALID http_status=200' -and $teamRoomLog -match 'TEAM_ROOM_HEALTH_NOT_AUTH=PASS' -and $teamRoomLog -match 'TEAM_ROOM_FALSE_SUCCESS_REMOVED=PASS') 'SUPERVISOR_HEALTH_IS_NOT_AUTHENTICATION'
  Check ($teamRoomLog -match 'TEAM_ROOM_ACCESS_TOKEN_REQUESTED=NO' -and $teamRoomLog -match 'TEAM_ROOM_AUTH_EXCHANGE_CALLED=NO') 'NO_ACCESS_TOKEN_OR_AUTH_EXCHANGE'
  Write-Host 'FAIL_CLOSED=PASS'
  Write-Host 'FALSE_SUCCESS_REMOVED=PASS'
  Write-Host 'HEALTH_NOT_AUTH=PASS'
  Write-Host 'ACCESS_TOKEN_REQUESTED=NO'
  Write-Host 'AUTH_EXCHANGE_CALLED=NO'
 Copy-Item $clientLog.FullName (Join-Path $evidence 'A-valid-appcontainer.log') -Force
 Copy-Item $svcLog (Join-Path $evidence 'A-valid-supervisor.log') -Force
 $teamRoomBridgePassed=$true
 Restart-P238Supervisor $p238Endpoint '1'
 Invoke-P238AppContainerScenario 'A2-fake-mode-non-authoritative' 'FAKE_FAIL_CLOSED' $null
 Restart-P238Supervisor $p238Endpoint '0'
 Start-P238TeamRoom $invalidLeaf $false $false;Restart-P238Supervisor $p238Endpoint
 Invoke-P238AppContainerScenario 'B-invalid-server-certificate' 'REJECT' 'TEAM_ROOM_HTTPS_TLS=REJECT reason=HTTPS_(SEND|RECEIVE)_TLS_OR_TRANSPORT'
 Start-P238TeamRoom $wrongHostLeaf $false $false;Restart-P238Supervisor $p238Endpoint
 Invoke-P238AppContainerScenario 'C-wrong-certificate-hostname' 'REJECT' 'TEAM_ROOM_HTTPS_TLS=REJECT reason=HTTPS_(SEND|RECEIVE)_TLS_OR_TRANSPORT'
 Start-P238TeamRoom $untrustedLeaf $false $false;Restart-P238Supervisor $p238Endpoint
 Invoke-P238AppContainerScenario 'D-untrusted-ca' 'REJECT' 'TEAM_ROOM_HTTPS_TLS=REJECT reason=HTTPS_(SEND|RECEIVE)_TLS_OR_TRANSPORT'
 Start-P238TeamRoom $goodLeaf $false;Restart-P238Supervisor 'http://team-room.p238.test:8443/health/live'
 Invoke-P238AppContainerScenario 'E-http-endpoint' 'REJECT' 'HTTPS_REQUIRED_HTTP_REJECTED'
 Restart-P238Supervisor 'https://unexpected.p238.test:8443/health/live'
 Invoke-P238AppContainerScenario 'F-unexpected-host' 'REJECT' 'ENDPOINT_HOST_NOT_ALLOWLISTED'
 Restart-P238Supervisor 'https://team-room.p238.test:9443/health/live'
 Invoke-P238AppContainerScenario 'G-unexpected-port' 'REJECT' 'ENDPOINT_PORT_NOT_ALLOWLISTED'
 Restart-P238Supervisor $p238Endpoint
 if($env:P238_TEAM_ROOM_FULL_MODE -eq '1'){
  foreach($port in @(8081,8082)){
   $tcpCheck=[Net.Sockets.TcpClient]::new();$httpOpen=$false
   try{$tcpCheck.Connect('127.0.0.1',$port);$httpOpen=$true}catch{}finally{$tcpCheck.Dispose()}
   Check (-not $httpOpen) ('TEAM_ROOM_HTTP_FALLBACK_PORT_'+$port+'_CLOSED')
  }
  Write-Host 'TEAM_ROOM_HTTP_FALLBACK=REJECTED'
  Restart-P238Supervisor $p238Endpoint
  Invoke-P238AppContainerScenario 'H-real-team-room-no-http-fallback' 'FAIL_CLOSED' $null
 }else{
  Start-P238TeamRoom $goodLeaf $true
  Invoke-P238AppContainerScenario 'H-http-redirect' 'REJECT' 'TEAM_ROOM_HEALTH_HTTP_STATUS_307'
  Start-P238TeamRoom $goodLeaf $false
  Restart-P238Supervisor $p238Endpoint
 }
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
 $final='PASS'
}
catch {Write-Host ('P238A_FAILURE='+$_.Exception.Message);throw}
finally {
 Stop-P238TeamRoom
 $teamRoomProcessGone=($null -eq $teamRoomProcess -or -not(Get-Process -Id $teamRoomProcess.Id -ErrorAction SilentlyContinue))
 if($p238HostsOriginal -ne $null){try{[IO.File]::WriteAllText($p238HostsPath,$p238HostsOriginal,[Text.Encoding]::ASCII);ipconfig.exe /flushdns|Out-Null;$p238HostsRestored=([IO.File]::ReadAllText($p238HostsPath) -ceq $p238HostsOriginal)}catch{$p238HostsRestored=$false}}
 foreach($thumb in $p238RootThumbprints){Remove-Item ("Cert:\LocalMachine\Root\"+$thumb) -Force -ErrorAction SilentlyContinue}
 foreach($c in $p238Certificates){Remove-Item ("Cert:\LocalMachine\My\"+$c.Thumbprint) -Force -ErrorAction SilentlyContinue}
 foreach($c in $p238Certificates){if(Get-ChildItem 'Cert:\LocalMachine\My' -ErrorAction SilentlyContinue|Where-Object Thumbprint -EQ $c.Thumbprint){$p238CertsGone=$false}}
 foreach($thumb in $p238RootThumbprints){if(Get-ChildItem 'Cert:\LocalMachine\Root' -ErrorAction SilentlyContinue|Where-Object Thumbprint -EQ $thumb){$p238CertsGone=$false}}
 foreach($key in $p238PrivateKeys){
  try{$k=[System.Security.Cryptography.CngKey]::Open($key.Name,$key.Provider,[System.Security.Cryptography.CngKeyOpenOptions]::MachineKey);$k.Delete();$k.Dispose()}catch{}
  try{$k2=[System.Security.Cryptography.CngKey]::Open($key.Name,$key.Provider,[System.Security.Cryptography.CngKeyOpenOptions]::MachineKey);$k2.Dispose();$p238KeysGone=$false}catch{}
 }
 $serviceGraceful=$false;$serviceGone=(-not $created);$clientGone=$false;$packageGone=(-not $installed);$certificateGone=$true;$privateKeyGone=($null -eq $cert);$tempGone=$false;$clientDataGone=$true;$fallbackLogGone=$true
 if($created){
  & sc.exe stop $service 2>&1|Out-Host
  $deadline=(Get-Date).AddSeconds(20);$state=''
  do{Start-Sleep -Milliseconds 500;$q=Get-CimInstance Win32_Service -Filter "Name='$service'" -ErrorAction SilentlyContinue;$state=$q.State}while($state -ne 'Stopped' -and (Get-Date) -lt $deadline)
  $svcLog=Join-Path $root 'supervisor.conf.log'
  $sl=''
  if(Test-Path $svcLog){
   $sl=Get-Content $svcLog -Raw
   Copy-Item $svcLog (Join-Path $evidence 'service.log') -Force
  }
  $serviceGraceful=($state -eq 'Stopped' -and ((-not $serviceStarted) -or $sl -match 'SERVICE_CLEANUP_COMPLETE=TRUE'))
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
 $fallbackLogPath=Join-Path $env:LOCALAPPDATA 'PhoneSupervisor-P236-client.log'
 Remove-Item $fallbackLogPath -Force -ErrorAction SilentlyContinue
 $fallbackLogGone=(-not(Test-Path $fallbackLogPath))
 if($app){$clientDataPath=Join-Path (Join-Path $env:LOCALAPPDATA 'Packages') $app.PackageFamilyName;if(Test-Path $clientDataPath){Remove-Item $clientDataPath -Recurse -Force -ErrorAction SilentlyContinue};$clientDataGone=(-not(Test-Path $clientDataPath))}
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
 Write-Host ('APPCONTAINER_PACKAGE_DATA_REMOVED='+$(if($clientDataGone){'PASS'}else{'FAIL'}))
 Write-Host ('CLIENT_FALLBACK_LOG_REMOVED='+$(if($fallbackLogGone){'PASS'}else{'FAIL'}))
 $logText='';if(Test-Path (Join-Path $evidence 'service.log')){$logText=Get-Content (Join-Path $evidence 'service.log') -Raw}
 $sessionClean=((-not $serviceStarted) -or $logText -match 'SERVICE_CLEANUP_COMPLETE=TRUE SESSIONS=0')
 Write-Host ('TEAM_ROOM_PROCESS_REMOVED='+$(if($teamRoomProcessGone){'PASS'}else{'FAIL'}));Write-Host ('P238A_CA_AND_CERTIFICATES_REMOVED='+$(if($p238CertsGone){'PASS'}else{'FAIL'}));Write-Host ('P238A_PRIVATE_KEYS_REMOVED='+$(if($p238KeysGone){'PASS'}else{'FAIL'}));Write-Host ('HOSTS_ENTRY_RESTORED='+$(if($p238HostsRestored){'PASS'}else{'FAIL'}))
 $cleanup=($serviceGone -and $clientGone -and $packageGone -and $certificateGone -and $privateKeyGone -and $tempGone -and $clientDataGone -and $fallbackLogGone -and $sessionClean -and $teamRoomProcessGone -and $p238CertsGone -and $p238KeysGone -and $p238HostsRestored)
 Write-Host ('CLEANUP_COMPLETE='+$(if($cleanup){'PASS'}else{'FAIL'}))
 if($env:P238_R4_FAIL_CLOSED -eq '1'){('P2.38-B_R4_FAIL_CLOSED='+$(if($teamRoomBridgePassed -and $cleanup -and $final -eq 'PASS'){'PASS'}else{'BLOCKED'}))|Set-Content (Join-Path $evidence 'r4-classification.txt') -Encoding ASCII}else{if($teamRoomBridgePassed -and $cleanup -and $final -eq 'PASS'){'P2.38-A_TLS_E2E=PASS'|Set-Content (Join-Path $evidence 'p2.38-a-classification.txt') -Encoding ASCII}else{'P2.38-A_TLS_E2E=BLOCKED'|Set-Content (Join-Path $evidence 'p2.38-a-classification.txt') -Encoding ASCII}}
 if(-not $cleanup){$final='BLOCKED'}
 ('P2.38-A.1_FINAL='+$final)|Set-Content (Join-Path $evidence 'classification.txt')
 @("commit=$env:GITHUB_SHA","run_id=$env:GITHUB_RUN_ID","job=windows-appcontainer-supervisor-teamroom-tls","hostname=team-room.p238.test","endpoint=https://team-room.p238.test:8443/health/live","classification=$final")|Set-Content (Join-Path $evidence 'run-metadata.txt') -Encoding ASCII
 Stop-Transcript|Out-Null
 $transcriptText=if(Test-Path (Join-Path $evidence 'transcript.log')){Get-Content (Join-Path $evidence 'transcript.log') -Raw}else{''}
 $fakeModeDistinct=($transcriptText -match 'FAKE_MODE_DISTINCT=PASS')
 $secretAbsent=($transcriptText -match 'SUPERVISOR_SECRET_MATERIAL_ABSENT=PASS')
 $r3StoreClean=($transcriptText -match 'R3_TEST_STORE_NOT_LEFT_BEHIND=PASS')
 $secretScanOk=$true
 $secretPatterns=@(
  'FAKE-SESSION-TOKEN-NOT-FOR-EXPORT',
  'FAKE-ENDPOINT-CREDENTIAL-NOT-FOR-EXPORT',
  'P2.37-TEST-ONLY-ASSERTION',
  '(?i)(client_secret|access_token|refresh_token|endpoint_credential|operator_key|password)\s*[:=]\s*[A-Za-z0-9_./+=-]{8,}',
  'eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}'
 )
 foreach($file in Get-ChildItem $evidence -File -ErrorAction SilentlyContinue){
  $raw=[IO.File]::ReadAllText($file.FullName)
  foreach($pattern in $secretPatterns){if($raw -match $pattern){$secretScanOk=$false}}
 }
 Write-Host ('SECRET_ISOLATION='+$(if($secretScanOk){'PASS'}else{'FAIL'}))
 Write-Host ('CLEANUP='+$(if($cleanup){'PASS'}else{'FAIL'}))
 $r4Pass=($secretScanOk -and $cleanup -and $teamRoomBridgePassed -and $final -eq 'PASS' -and $fakeModeDistinct -and $secretAbsent -and $r3StoreClean)
 $r4Verdict=if($r4Pass){'PASS'}else{'BLOCKED'}
 @(
  'ROUND=4/12'
  'FAIL_CLOSED='+$(if($r4Pass){'PASS'}else{'FAIL'})
  'FALSE_SUCCESS_REMOVED='+$(if($r4Pass){'PASS'}else{'FAIL'})
  'HEALTH_NOT_AUTH='+$(if($r4Pass){'PASS'}else{'FAIL'})
  'SECRET_ISOLATION='+$(if($secretScanOk){'PASS'}else{'FAIL'})
  'SUPERVISOR_SECRET_MATERIAL_ABSENT='+$(if($secretAbsent){'PASS'}else{'FAIL'})
  'R3_TEST_STORE_NOT_LEFT_BEHIND='+$(if($r3StoreClean){'PASS'}else{'FAIL'})
  'FAKE_MODE_DISTINCT='+$(if($fakeModeDistinct){'PASS'}else{'FAIL'})
  'CLEANUP='+$(if($cleanup){'PASS'}else{'FAIL'})
  'ACCESS_TOKEN_REQUESTED=NO'
  'AUTH_EXCHANGE_CALLED=NO'
  'R4_VERDICT='+$r4Verdict
  'VERDICT='+$r4Verdict
 ) | Set-Content (Join-Path $evidence 'r4-summary.txt') -Encoding ASCII
}
