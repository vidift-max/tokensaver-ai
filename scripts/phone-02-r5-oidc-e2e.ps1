param(
 [Parameter(Mandatory=$true)][string]$SupervisorExe,
 [Parameter(Mandatory=$true)][string]$OidcDll,
 [Parameter(Mandatory=$true)][string]$ClientExe
)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$root=Join-Path $env:RUNNER_TEMP 'phone02-r5-oidc'
$evidence=Join-Path $env:RUNNER_TEMP 'phone02-r5-evidence'
$serviceDir=Join-Path $root 'service'
$service='TokenSaverPhone02Supervisor'
$realmName='phone02-p238b'
$clientId='phone02-supervisor-p238b'
$scopeName='team_room:connect'
$audience='team-room-api'
$issuer="https://keycloak.p238.test:8443/realms/$realmName"
$base='https://keycloak.p238.test:8443'
$pgDatabase="keycloak_p238b_r5_$env:GITHUB_RUN_ID"
$pgStartedByUs=$false
$pgPassword=[Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
$adminPassword=[Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
$adminUser='r5-bootstrap-admin'
$oidcClientSecret=$null;$secretBytes=$null;$adminToken=$null;$adminResponse=$null;$secretInfo=$null
$pgService=$null;$pgInstalled=$false;$keycloakProcess=$null;$badTlsProcess=$null;$serviceCreated=$false
$rootThumbprint=$null;$hostsPath=Join-Path $env:windir 'System32\drivers\etc\hosts';$hostsOriginal=$null
$storeRoot="C:\ProgramData\TokenSaver\Phone02\CredentialStoreR5-$env:GITHUB_RUN_ID"
$checks=[System.Collections.Generic.List[string]]::new()
$cleanup=[System.Collections.Generic.List[string]]::new()
$failure=$null;$svcLog=$null;$clientLog=$null;$psql=$null;$kcHome=$null
New-Item -ItemType Directory -Force -Path $root,$evidence,$serviceDir | Out-Null
function Mark([string]$name,[bool]$ok){
 $value=$name+'='+$(if($ok){'PASS'}else{'FAIL'})
 Write-Host $value
 $checks.Add($value)
 if(-not $ok){throw ($name+' failed')}
}
function Wait-LogMarker([string]$path,[string]$marker,[int]$seconds=60){
 $deadline=(Get-Date).AddSeconds($seconds)
 do{
  if(Test-Path $path){
   $body=Get-Content $path -Raw -ErrorAction SilentlyContinue
   if($body -and $body.Contains($marker)){return $body}
  }
  Start-Sleep -Milliseconds 250
 }while((Get-Date)-lt $deadline)
 throw ('LOG_MARKER_TIMEOUT_'+$marker)
}
function Wait-TcpPort([string]$hostName,[int]$port,[int]$seconds=30,[bool]$expected=$true){
 $deadline=(Get-Date).AddSeconds($seconds)
 do{
  $open=$false
  try{$tcp=[Net.Sockets.TcpClient]::new();$ar=$tcp.BeginConnect($hostName,$port,$null,$null);$open=$ar.AsyncWaitHandle.WaitOne(500) -and $tcp.Connected;$tcp.Close()}catch{}
  if($open -eq $expected){return $true}
  Start-Sleep -Milliseconds 250
 }while((Get-Date)-lt $deadline)
 return $false
}
function Stop-Keycloak($process,[string]$kcHomePath){
 if($kcHomePath){
  $children=@(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue|Where-Object {$_.CommandLine -and $_.CommandLine.Contains($kcHomePath)})
  foreach($child in $children){Stop-Process -Id $child.ProcessId -Force -ErrorAction SilentlyContinue}
 }
 if($process -and (Get-Process -Id $process.Id -ErrorAction SilentlyContinue)){Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue}
 if($process){Wait-Process -Id $process.Id -Timeout 15 -ErrorAction SilentlyContinue}
}
function Export-SanitizedKeycloakStartup([string]$stdoutPath,[string]$stderrPath){
 $diagnostic=Join-Path $evidence 'keycloak-startup-diagnostic.txt'
 $lines=[System.Collections.Generic.List[string]]::new()
 foreach($path in @($stdoutPath,$stderrPath)){
  if(Test-Path $path){
   $lines.Add(('SOURCE='+$(if($path -eq $stdoutPath){'stdout'}else{'stderr'})))
   foreach($line in (Get-Content $path -Tail 100 -ErrorAction SilentlyContinue)){
    $safe=[string]$line
    $safe=$safe -replace '(?i)(password|client_secret|secret|access_token|refresh_token|authorization|credential)\s*[:=]\s*[^\s,;]+','$1=[REDACTED]'
    $safe=$safe -replace '(?i)eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}','[JWT_REDACTED]'
    $safe=$safe -replace '(?i)(Bearer\s+)[A-Za-z0-9._~+/-]+=*','$1[REDACTED]'
    $lines.Add($safe)
   }
  }
 }
 $lines | Set-Content $diagnostic -Encoding utf8
 Write-Host 'KEYCLOAK_STARTUP_DIAGNOSTIC_EXPORTED=PASS'
}
function Start-R5Keycloak([string]$kcBat,[string]$kcHomePath,[string]$cert,[string]$key,[bool]$importRealm,[bool]$bootstrapAdmin){
 $env:KC_HEALTH_ENABLED='true';$env:KC_HTTP_ENABLED='false';$env:KC_HTTPS_PORT='8443'
 $env:KC_HTTPS_CERTIFICATE_FILE=$cert.Replace('\','/');$env:KC_HTTPS_CERTIFICATE_KEY_FILE=$key.Replace('\','/')
 $env:KC_HOSTNAME='https://keycloak.p238.test:8443';$env:KC_HOSTNAME_STRICT='true'
 $env:KC_DB='postgres';$env:KC_DB_URL_HOST='127.0.0.1';$env:KC_DB_URL_PORT='5432';$env:KC_DB_URL_DATABASE=$pgDatabase
 $env:KC_DB_USERNAME='postgres';$env:KC_DB_PASSWORD=$pgPassword
 if($bootstrapAdmin){$env:KC_BOOTSTRAP_ADMIN_USERNAME=$adminUser;$env:KC_BOOTSTRAP_ADMIN_PASSWORD=$adminPassword}
 $kcArgs='start-dev --db=postgres --http-enabled=false --hostname-strict=true'
 if($importRealm){$kcArgs+=' --import-realm'}
 $stamp=Get-Date -Format 'HHmmss'
 $out=Join-Path $root ('keycloak-'+$stamp+'.stdout.log')
 $err=Join-Path $root ('keycloak-'+$stamp+'.stderr.log')
 $p=Start-Process -FilePath $kcBat -ArgumentList $kcArgs -WorkingDirectory $kcHomePath -PassThru -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError $err
 Remove-Item Env:KC_HEALTH_ENABLED,Env:KC_HTTP_ENABLED,Env:KC_HTTPS_PORT,Env:KC_HTTPS_CERTIFICATE_FILE,Env:KC_HTTPS_CERTIFICATE_KEY_FILE,Env:KC_HOSTNAME,Env:KC_HOSTNAME_STRICT,Env:KC_DB,Env:KC_DB_URL_HOST,Env:KC_DB_URL_PORT,Env:KC_DB_URL_DATABASE,Env:KC_DB_USERNAME,Env:KC_DB_PASSWORD,Env:KC_BOOTSTRAP_ADMIN_USERNAME,Env:KC_BOOTSTRAP_ADMIN_PASSWORD -ErrorAction SilentlyContinue
 $deadline=(Get-Date).AddSeconds(240);$ready=$false
 do{
  try{$d=Invoke-RestMethod -Uri "$issuer/.well-known/openid-configuration" -TimeoutSec 4;if($d.issuer -ceq $issuer){$ready=$true}}catch{}
  if(-not $ready){Start-Sleep -Seconds 2}
 }while(-not $ready -and (Get-Date)-lt $deadline)
 if(-not $ready){Export-SanitizedKeycloakStartup $out $err;throw 'KEYCLOAK_HTTPS_READINESS_TIMEOUT'}
 return $p
}
function Find-Bytes([byte[]]$haystack,[byte[]]$needle){
 if(!$needle -or $needle.Length -eq 0 -or $haystack.Length -lt $needle.Length){return $false}
 for($i=0;$i -le $haystack.Length-$needle.Length;$i++){
  $match=$true
  for($j=0;$j -lt $needle.Length;$j++){if($haystack[$i+$j] -ne $needle[$j]){$match=$false;break}}
  if($match){return $true}
 }
 return $false
}
try{
 Mark 'EXPLICIT_BRANCH_GUARD' ($env:GITHUB_REF -eq 'refs/heads/mission/phone-02-p238-b-oidc')
 Mark 'WINDOWS_X64_RUNNER' ($env:RUNNER_OS -eq 'Windows' -and $env:RUNNER_ARCH -eq 'X64')
 Mark 'BUILD_OUTPUTS_PRESENT' ((Test-Path $SupervisorExe) -and (Test-Path $OidcDll) -and (Test-Path $ClientExe))
 Mark 'R5_STORE_PATH_FRESH' (-not(Test-Path $storeRoot))
 $openssl=(Get-Command openssl -ErrorAction Stop).Source
 $chocoArgs=@('install','postgresql16','--version=16.15.0','--params',("/Password:$pgPassword /Port:5432 /NoPath"),'--yes','--no-progress','--limit-output')
 & choco @chocoArgs
 if($LASTEXITCODE -ne 0){throw 'POSTGRESQL_INSTALL_FAILED'}
 $pgInstalled=$true
 $pgService=Get-Service|Where-Object {$_.Name -match '^postgresql.*16'}|Select-Object -First 1
 if(-not $pgService){throw 'POSTGRESQL_SERVICE_MISSING'}
 if($pgService.Status -ne 'Running'){Start-Service -Name $pgService.Name;$pgStartedByUs=$true}
 $psql=Join-Path $env:ProgramFiles 'PostgreSQL\16\bin\psql.exe'
 if(-not(Test-Path $psql)){throw 'POSTGRESQL_PSQL_MISSING'}
 $pgDeadline=(Get-Date).AddSeconds(60);$pgReady=$false
 do{$env:PGPASSWORD=$pgPassword;& $psql -h 127.0.0.1 -p 5432 -U postgres -d postgres -v ON_ERROR_STOP=1 -Atc 'SELECT 1' 2>$null;$pgReady=($LASTEXITCODE -eq 0);if(!$pgReady){Start-Sleep -Seconds 2}}while(!$pgReady -and (Get-Date)-lt $pgDeadline)
 Mark 'POSTGRESQL_NATIVE_SERVICE_READY' $pgReady
 $env:PGPASSWORD=$pgPassword
 & $psql -h 127.0.0.1 -p 5432 -U postgres -d postgres -v ON_ERROR_STOP=1 -c "CREATE DATABASE $pgDatabase" 2>&1|Out-Null
 if($LASTEXITCODE -ne 0){throw 'KEYCLOAK_DATABASE_CREATE_FAILED'}
 Remove-Item Env:PGPASSWORD -ErrorAction SilentlyContinue

 $kcZip=Join-Path $root 'keycloak-26.3.5.zip';$kcHome=Join-Path $root 'keycloak-26.3.5'
 Invoke-WebRequest -Uri 'https://github.com/keycloak/keycloak/releases/download/26.3.5/keycloak-26.3.5.zip' -OutFile $kcZip
 $kcHash=(Get-FileHash $kcZip -Algorithm SHA256).Hash.ToLower()
 Expand-Archive -Path $kcZip -DestinationPath $root -Force
 $kcBat=Join-Path $kcHome 'bin\kc.bat'
 Mark 'KEYCLOAK_DISTRIBUTION_PRESENT' (Test-Path $kcBat)
 Write-Host ('KEYCLOAK_ARCHIVE_SHA256='+$kcHash)

 $certDir=Join-Path $root 'tls';New-Item -ItemType Directory -Force -Path $certDir|Out-Null
 $goodCaKey=Join-Path $certDir 'good-ca.key';$goodCaCert=Join-Path $certDir 'good-ca.crt'
 $goodKey=Join-Path $certDir 'keycloak-good.key';$goodCsr=Join-Path $certDir 'keycloak-good.csr';$goodCert=Join-Path $certDir 'keycloak-good.crt'
 $badCaKey=Join-Path $certDir 'bad-ca.key';$badCaCert=Join-Path $certDir 'bad-ca.crt'
 $badKey=Join-Path $certDir 'keycloak-bad.key';$badCsr=Join-Path $certDir 'keycloak-bad.csr';$badCert=Join-Path $certDir 'keycloak-bad.crt'
 $ext=Join-Path $certDir 'keycloak-ext.cnf'
 $nl=[Environment]::NewLine
 Set-Content -LiteralPath $ext -Value ("[v3_req]"+$nl+"subjectAltName=DNS:keycloak.p238.test"+$nl+"extendedKeyUsage=serverAuth"+$nl+"keyUsage=digitalSignature,keyEncipherment"+$nl+"basicConstraints=CA:FALSE"+$nl) -Encoding ascii
 & $openssl req -x509 -newkey rsa:2048 -nodes -keyout $goodCaKey -out $goodCaCert -days 2 -subj '/CN=Phone02 R5 Trusted Test CA' -addext 'basicConstraints=critical,CA:TRUE' -addext 'keyUsage=critical,keyCertSign,cRLSign' 2>&1|Out-Null
 if($LASTEXITCODE -ne 0){throw 'R5_GOOD_CA_GENERATION_FAILED'}
 & $openssl req -newkey rsa:2048 -nodes -keyout $goodKey -out $goodCsr -subj '/CN=keycloak.p238.test' 2>&1|Out-Null
 if($LASTEXITCODE -ne 0){throw 'R5_GOOD_LEAF_CSR_FAILED'}
 & $openssl x509 -req -in $goodCsr -CA $goodCaCert -CAkey $goodCaKey -CAcreateserial -out $goodCert -days 2 -extfile $ext -extensions v3_req 2>&1|Out-Null
 if($LASTEXITCODE -ne 0){throw 'R5_GOOD_LEAF_CERT_FAILED'}
 & $openssl req -x509 -newkey rsa:2048 -nodes -keyout $badCaKey -out $badCaCert -days 2 -subj '/CN=Phone02 R5 Untrusted Test CA' -addext 'basicConstraints=critical,CA:TRUE' -addext 'keyUsage=critical,keyCertSign,cRLSign' 2>&1|Out-Null
 if($LASTEXITCODE -ne 0){throw 'R5_BAD_CA_GENERATION_FAILED'}
 & $openssl req -newkey rsa:2048 -nodes -keyout $badKey -out $badCsr -subj '/CN=keycloak.p238.test' 2>&1|Out-Null
 if($LASTEXITCODE -ne 0){throw 'R5_BAD_LEAF_CSR_FAILED'}
 & $openssl x509 -req -in $badCsr -CA $badCaCert -CAkey $badCaKey -CAcreateserial -out $badCert -days 2 -extfile $ext -extensions v3_req 2>&1|Out-Null
 if($LASTEXITCODE -ne 0){throw 'R5_BAD_LEAF_CERT_FAILED'}
 $imported=Import-Certificate -FilePath $goodCaCert -CertStoreLocation 'Cert:\LocalMachine\Root'
 $rootThumbprint=$imported.Thumbprint
 Mark 'KEYCLOAK_TEST_CA_TRUSTED' ([bool]$rootThumbprint)
 $hostsOriginal=[IO.File]::ReadAllText($hostsPath)
 if($hostsOriginal -notmatch '(?im)^\s*127\.0\.0\.1\s+keycloak\.p238\.test\s*$'){[IO.File]::AppendAllText($hostsPath,($nl+'127.0.0.1 keycloak.p238.test'+$nl))}
 Mark 'KEYCLOAK_HOSTNAME_RESOLVES_LOCAL' ([Net.Dns]::GetHostAddresses('keycloak.p238.test').IPAddressToString -contains '127.0.0.1')

 $importDir=Join-Path $kcHome 'data\import';New-Item -ItemType Directory -Force -Path $importDir|Out-Null
 $importFile=Join-Path $importDir 'phone02-p238b-realm.json'
 $realm=[ordered]@{
  realm=$realmName;enabled=$true;displayName='Phone02 R5 isolated OIDC test realm';sslRequired='all';defaultSignatureAlgorithm='RS256'
  clientScopes=@(@{name=$scopeName;description='Phone02 R5 Team Room connection scope';protocol='openid-connect';attributes=@{'include.in.token.scope'='true';'display.on.consent.screen'='false'};protocolMappers=@(@{name='team-room-api-audience';protocol='openid-connect';protocolMapper='oidc-audience-mapper';config=@{'included.client.audience'=$audience;'id.token.claim'='false';'access.token.claim'='true'}})})
  clients=@(@{clientId=$clientId;name='Phone02 R5 Supervisor test client';enabled=$true;protocol='openid-connect';publicClient=$false;bearerOnly=$false;clientAuthenticatorType='client-secret';serviceAccountsEnabled=$true;standardFlowEnabled=$false;implicitFlowEnabled=$false;directAccessGrantsEnabled=$false;authorizationServicesEnabled=$false;defaultClientScopes=@($scopeName,'profile','email','roles','web-origins');optionalClientScopes=@();attributes=@{'oauth2.device.authorization.grant.enabled'='false';'oidc.ciba.grant.enabled'='false'}})
 }
 $realmJson=$realm|ConvertTo-Json -Depth 30
 if($realmJson -match '(?i)"secret"\s*:'){throw 'R5_REALM_IMPORT_MUST_NOT_CONTAIN_CLIENT_SECRET'}
 Set-Content -LiteralPath $importFile -Value $realmJson -Encoding utf8
 $realm=$null;$realmJson=$null
 $keycloakProcess=Start-R5Keycloak $kcBat $kcHome $goodCert $goodKey $true $true
 Mark 'KEYCLOAK_HTTPS' (Test-NetConnection 127.0.0.1 -Port 8443 -InformationLevel Quiet)
 $httpListeners=@(Get-NetTCPConnection -State Listen -LocalPort 8080 -ErrorAction SilentlyContinue)
 $kcHttpListener=$false;$httpListenerOwnerUnknown=$false
 foreach($listener in $httpListeners){
  $owner=Get-CimInstance Win32_Process -Filter "ProcessId=$($listener.OwningProcess)" -ErrorAction SilentlyContinue
  $ownerName=if($owner){$owner.Name}else{'UNKNOWN'}
  Write-Host ('HTTP_PORT_8080_LISTENER=PID:'+ $listener.OwningProcess + ';PROCESS:'+ $ownerName)
  if(-not $owner -or [string]::IsNullOrWhiteSpace([string]$owner.CommandLine)){
   $httpListenerOwnerUnknown=$true
  }elseif($owner.CommandLine.Contains($kcHome)){$kcHttpListener=$true}
 }
 Mark 'KEYCLOAK_HTTP_LISTENER_OWNER_UNKNOWN' (-not $httpListenerOwnerUnknown)
 Mark 'KEYCLOAK_HTTP_DISABLED' (-not $kcHttpListener -and -not $httpListenerOwnerUnknown)
 $discovery=Invoke-RestMethod -Uri "$issuer/.well-known/openid-configuration" -TimeoutSec 15
 Mark 'OIDC_DISCOVERY' ($discovery.issuer -ceq $issuer -and $discovery.token_endpoint -ceq "$issuer/protocol/openid-connect/token" -and $discovery.jwks_uri -ceq "$issuer/protocol/openid-connect/certs")
 Mark 'OIDC_ISSUER_VALID' ($discovery.issuer -ceq $issuer)
 $jwks=Invoke-RestMethod -Uri $discovery.jwks_uri -TimeoutSec 15
 Mark 'OIDC_JWKS_RSA_PRESENT' (@($jwks.keys|Where-Object {$_.kty -eq 'RSA' -and $_.use -eq 'sig' -and $_.kid}).Count -gt 0)

 $adminForm=@{grant_type='password';client_id='admin-cli';username=$adminUser;password=$adminPassword}
 $adminResponse=Invoke-RestMethod -Uri "$base/realms/master/protocol/openid-connect/token" -Method Post -ContentType 'application/x-www-form-urlencoded' -Body $adminForm -TimeoutSec 15
 $adminToken=[string]$adminResponse.access_token
 if([string]::IsNullOrWhiteSpace($adminToken)){throw 'KEYCLOAK_ADMIN_TOKEN_MISSING'}
 $headers=@{Authorization=("Bearer "+$adminToken)}
 $clients=@(Invoke-RestMethod -Uri "$base/admin/realms/$realmName/clients?clientId=$clientId" -Headers $headers -TimeoutSec 15)
 $clientObject=$clients|Where-Object {$_.clientId -eq $clientId}|Select-Object -First 1
 if(-not $clientObject -or $clientObject.publicClient -or -not $clientObject.serviceAccountsEnabled -or $clientObject.clientAuthenticatorType -ne 'client-secret'){throw 'KEYCLOAK_CLIENT_CONFIGURATION_INVALID'}
 $clientUuid=[string]$clientObject.id
 try{$secretInfo=Invoke-RestMethod -Uri "$base/admin/realms/$realmName/clients/$clientUuid/client-secret" -Headers $headers -TimeoutSec 15}catch{$secretInfo=$null}
 if(-not $secretInfo -or [string]::IsNullOrWhiteSpace([string]$secretInfo.value)){
  $null=Invoke-RestMethod -Uri "$base/admin/realms/$realmName/clients/$clientUuid/client-secret" -Method Post -Headers $headers -TimeoutSec 15
  $secretInfo=Invoke-RestMethod -Uri "$base/admin/realms/$realmName/clients/$clientUuid/client-secret" -Headers $headers -TimeoutSec 15
 }
 $oidcClientSecret=[string]$secretInfo.value
 if($oidcClientSecret.Length -lt 32 -or $oidcClientSecret.Length -gt 256){throw 'KEYCLOAK_CLIENT_SECRET_LENGTH_INVALID'}
 Mark 'KEYCLOAK_CLIENT_CONFIDENTIAL' $true
 Mark 'KEYCLOAK_SERVICE_ACCOUNT_ENABLED' $true
 Mark 'KEYCLOAK_SCOPE_AND_AUDIENCE_CONFIGURED' ($clientObject.defaultClientScopes -contains $scopeName)
 $realmText=[IO.File]::ReadAllText($importFile)
 Mark 'CLIENT_SECRET_NOT_IN_REALM_IMPORT_FILE' (-not $realmText.Contains($oidcClientSecret))
 $env:PGPASSWORD=$pgPassword
 $sqlRealm=& $psql -h 127.0.0.1 -p 5432 -U postgres -d $pgDatabase -v ON_ERROR_STOP=1 -Atc "SELECT COUNT(*) FROM realm WHERE name='$realmName'" 2>$null
 $sqlClient=& $psql -h 127.0.0.1 -p 5432 -U postgres -d $pgDatabase -v ON_ERROR_STOP=1 -Atc "SELECT COUNT(*) FROM client WHERE client_id='$clientId' AND enabled=true AND public_client=false AND service_accounts_enabled=true" 2>$null
 Remove-Item Env:PGPASSWORD -ErrorAction SilentlyContinue
 Mark 'KEYCLOAK_POSTGRES_REALM_PERSISTED' ([int]$sqlRealm -eq 1 -and [int]$sqlClient -eq 1)
 Remove-Item $importFile -Force
 $adminToken=$null;$adminResponse=$null;$headers=$null;$secretInfo=$null;$clientObject=$null;$clients=$null;$adminForm=$null
 $adminPassword=$null
 Stop-Keycloak $keycloakProcess $kcHome
 $keycloakProcess=$null
 Mark 'KEYCLOAK_STOPPED_FOR_NEGATIVE_PROBES' (Wait-TcpPort '127.0.0.1' 8443 20 $false)
 $badOut=Join-Path $root 'bad-tls.stdout.log';$badErr=Join-Path $root 'bad-tls.stderr.log'
 $badTlsProcess=Start-Process -FilePath $openssl -ArgumentList @('s_server','-accept','8444','-cert',$badCert,'-key',$badKey,'-www','-quiet') -PassThru -WindowStyle Hidden -RedirectStandardOutput $badOut -RedirectStandardError $badErr
 Mark 'BAD_TLS_PROBE_LISTENER_READY' (Wait-TcpPort '127.0.0.1' 8444 20 $true)

 Copy-Item $SupervisorExe (Join-Path $serviceDir 'phone_supervisor.exe') -Force
 Copy-Item $OidcDll (Join-Path $serviceDir 'phone_supervisor_oidc_r5.dll') -Force
 Copy-Item $ClientExe (Join-Path $serviceDir 'phone_supervisor_client.exe') -Force
 $currentSid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
 $dummyAppSid='S-1-15-2-1'
 $config=Join-Path $serviceDir 'supervisor-r5.conf'
 $configLines=@(
  'PACKAGE_FAMILY=Phone02R5Dummy_1.0.0.0_x64__1234567890123'
  'PACKAGE_FULL=Phone02R5Dummy_1.0.0.0_x64__1234567890123'
  "APPCONTAINER_SID=$dummyAppSid"
  "RPC_CLIENT_APPCONTAINER_SID=$dummyAppSid"
  "RPC_CLIENT_USER_SID=$currentSid"
  "USER_SID=$currentSid"
  'SERVICE_SID=PLACEHOLDER'
  'TEAM_ROOM_FAKE_MODE=0'
  'TEAM_ROOM_TEST_AI_PHONE_ID=R5-TEST-PHONE-02'
  'TEAM_ROOM_TEST_ENDPOINT_ID=R5-TEST-ENDPOINT-02'
  'TEAM_ROOM_TEST_RECIPIENT_ID=R5-TEST-RECIPIENT-02'
  'TEAM_ROOM_HTTPS_ENDPOINT='
  'R5_TEST_MODE=1'
  "CREDENTIAL_STORE_ROOT=$storeRoot"
 )
 Set-Content -LiteralPath $config -Value $configLines -Encoding ascii
 $binPath='"'+(Join-Path $serviceDir 'phone_supervisor.exe')+'" --service "'+$config+'"'
 & sc.exe create $service 'binPath=' $binPath 'start=' 'demand' 'obj=' 'NT AUTHORITY\LocalService'|Out-Null
 if($LASTEXITCODE -ne 0){throw 'SUPERVISOR_SERVICE_CREATE_FAILED'}
 $serviceCreated=$true
 & sc.exe sidtype $service unrestricted|Out-Null
 if($LASTEXITCODE -ne 0){throw 'SUPERVISOR_SERVICE_SIDTYPE_FAILED'}
 $sidOutput=(& sc.exe showsid $service 2>&1|Out-String)
 $serviceSid=[regex]::Match($sidOutput,'S-1-5-80-(?:\d+-){4}\d+').Value
 if([string]::IsNullOrWhiteSpace($serviceSid)){throw 'SUPERVISOR_SERVICE_SID_UNAVAILABLE'}
 $configLines=$configLines|ForEach-Object {if($_ -eq 'SERVICE_SID=PLACEHOLDER'){"SERVICE_SID=$serviceSid"}else{$_}}
 Set-Content -LiteralPath $config -Value $configLines -Encoding ascii
 & sc.exe start $service|Out-Null
 if($LASTEXITCODE -ne 0){throw 'SUPERVISOR_SERVICE_START_FAILED'}
 $svcLog=$config+'.log'
 Wait-LogMarker $svcLog 'CREDENTIAL_PROVISIONING_PIPE=READY' 30|Out-Null
 Mark 'LOCAL_SERVICE' ((Get-Content $svcLog -Raw) -match 'LOCAL_SERVICE=PASS')
 Mark 'SERVICE_SID' ((Get-Content $svcLog -Raw) -match 'SERVICE_SID=PASS')
 Mark 'CREDENTIAL_STORE_INTEGRATED' ((Get-Content $svcLog -Raw) -match 'CREDENTIAL_STORE_INTEGRATED=PASS')

 $pipe=[IO.Pipes.NamedPipeClientStream]::new('.', 'TokenSaver.Phone02.CredentialProvisioning.R5', [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::None, [Security.Principal.TokenImpersonationLevel]::Impersonation)
 $pipe.Connect(15000)
 $secretBytes=[Text.Encoding]::UTF8.GetBytes($oidcClientSecret)
 $magic=[Text.Encoding]::ASCII.GetBytes('TSR5')
 $lengthBytes=[BitConverter]::GetBytes([uint32]$secretBytes.Length)
 $pipe.Write($magic,0,$magic.Length);$pipe.Write($lengthBytes,0,$lengthBytes.Length);$pipe.Write($secretBytes,0,$secretBytes.Length);$pipe.Flush()
 $ack=New-Object byte[] 4;$ackRead=$pipe.Read($ack,0,4);$pipe.Dispose()
 [Array]::Clear($secretBytes,0,$secretBytes.Length);$secretBytes=$null
 if($ackRead -ne 4 -or [BitConverter]::ToInt32($ack,0) -ne 0){throw 'ADMIN_PIPE_PROVISIONING_ACK_FAILED'}
 Wait-LogMarker $svcLog 'R5_WAITING_FOR_KEYCLOAK=PASS' 45|Out-Null
 Mark 'CREDENTIAL_PROVISIONING_ADMIN_ONLY' ((Get-Content $svcLog -Raw) -match 'CREDENTIAL_PROVISIONING_ADMIN_ONLY=PASS')
 Mark 'CREDENTIAL_PROTECTED' ((Get-Content $svcLog -Raw) -match 'CREDENTIAL_PROTECTED=PASS')

 $keycloakProcess=Start-R5Keycloak $kcBat $kcHome $goodCert $goodKey $false $false
 Wait-LogMarker $svcLog 'R5_VERDICT=PASS' 300|Out-Null
 Mark 'SUPERVISOR_OIDC_R5_VERDICT' ((Get-Content $svcLog -Raw) -match 'R5_VERDICT=PASS')
 $clientLog=Join-Path $env:LOCALAPPDATA 'PhoneSupervisor-P236-client.log'
 Remove-Item $clientLog -Force -ErrorAction SilentlyContinue
 $clientRun=Start-Process -FilePath (Join-Path $serviceDir 'phone_supervisor_client.exe') -ArgumentList '--common' -PassThru -Wait
 $clientBody=if(Test-Path $clientLog){Get-Content $clientLog -Raw}else{''}
 Mark 'COMMON_PROCESS_REJECT' ($clientRun.ExitCode -eq 0 -and $clientBody -match 'COMMON_PROCESS_REJECT=PASS')
 Mark 'COMMON_PROCESS_TEAM_ROOM_REJECT' ($clientBody -match 'COMMON_PROCESS_TEAM_ROOM_REJECT=PASS')

 $rpcIdl=Get-Content (Join-Path $PSScriptRoot '..\services\phone-supervisor\phone_supervisor_rpc.idl') -Raw
 $rpcClient=Get-Content (Join-Path $PSScriptRoot '..\services\phone-supervisor\phone_supervisor_client.cpp') -Raw
 $forbidden='(?i)(client_secret|access_token|bearer_token|team_room_bearer|endpoint_credential|session_token)'
 $rpcSafe=(-not($rpcIdl -match $forbidden) -and -not($rpcClient -match $forbidden))
 Mark 'RPC_TOKEN_EXPOSURE' $rpcSafe
 Mark 'SUPERVISOR_OIDC_TOKEN_EXCHANGE' ((Get-Content $svcLog -Raw) -match 'SUPERVISOR_OIDC_TOKEN_EXCHANGE=PASS')
 Mark 'RPC_AUTH_EXCHANGE_CALLED_NO' ((Get-Content $svcLog -Raw) -match 'RPC_AUTH_EXCHANGE_CALLED=NO')

 $processes=Get-CimInstance Win32_Process -ErrorAction SilentlyContinue
 $commandLineLeak=$false
 foreach($p in $processes){if($p.CommandLine -and $p.CommandLine.Contains($oidcClientSecret)){$commandLineLeak=$true}}
 $envLeak=$false
 foreach($item in [Environment]::GetEnvironmentVariables('Process').GetEnumerator()){if([string]$item.Value -ceq $oidcClientSecret){$envLeak=$true}}
 $fileLeak=$false;$jwtLeak=$false
 $storeFile=Join-Path $storeRoot 'oidc-client-secret.dpapi'
 $scanFiles=@(Get-ChildItem $root -File -Recurse -ErrorAction SilentlyContinue|Where-Object {$_.Extension -in @('.log','.json','.conf','.txt','.ini','.xml')})+@(Get-Item $svcLog,$clientLog,$storeFile -ErrorAction SilentlyContinue)
 $secretNeedle=[Text.Encoding]::UTF8.GetBytes($oidcClientSecret)
 foreach($file in $scanFiles|Where-Object {$_}){
  try{$bytes=[IO.File]::ReadAllBytes($file.FullName);if(Find-Bytes $bytes $secretNeedle){$fileLeak=$true};$text=[Text.Encoding]::UTF8.GetString($bytes);if($text -match 'eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}'){$jwtLeak=$true}}catch{}
 }
 [Array]::Clear($secretNeedle,0,$secretNeedle.Length)
 Mark 'CLIENT_SECRET_NOT_IN_CLI_ENV_FILE_RPC' (-not $commandLineLeak -and -not $envLeak -and -not $fileLeak -and $rpcSafe)
 Mark 'TOKEN_LOG_EXPOSURE' (-not $jwtLeak)
 $oidcClientSecret=$null
}
catch{
 $failure=$_
 Write-Host ('R5_RUNTIME_FAILURE_TYPE='+$_.Exception.GetType().FullName)
 Write-Host ('R5_RUNTIME_FAILURE_HRESULT='+('0x{0:X8}' -f $_.Exception.HResult))
 $safeMessage=[string]$_.Exception.Message
 $safeMessage=$safeMessage -replace '(?i)(password|secret|token|credential)\s*[:=]\s*[^\s,;]+','$1=[REDACTED]'
 $safeMessage=$safeMessage -replace '(?i)eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}','[JWT-REDACTED]'
 Write-Host ('R5_RUNTIME_FAILURE_MESSAGE='+$safeMessage)
}
finally{
 if($serviceCreated){
  try{
   & sc.exe stop $service|Out-Null
   $deadline=(Get-Date).AddSeconds(30)
   do{$svc=Get-Service -Name $service -ErrorAction SilentlyContinue;if(!$svc -or $svc.Status -eq 'Stopped'){break};Start-Sleep -Seconds 1}while((Get-Date)-lt $deadline)
   $svcBody=if(Test-Path $svcLog){Get-Content $svcLog -Raw}else{''}
   $serviceStopped=(!$svc -or $svc.Status -eq 'Stopped')
   $serviceCleanup=($svcBody -match 'TOKEN_MEMORY_ZEROED=PASS' -and $svcBody -match 'CREDENTIAL_REMOVE=PASS' -and $svcBody -match 'CLEANUP=PASS')
   $cleanup.Add('SUPERVISOR_SERVICE_STOP='+$(if($serviceStopped){'PASS'}else{'FAIL'}))
   $cleanup.Add('SUPERVISOR_TOKEN_AND_STORE_CLEANUP='+$(if($serviceCleanup){'PASS'}else{'FAIL'}))
   & sc.exe delete $service|Out-Null
   Start-Sleep -Seconds 2
   $serviceAbsent=$null -eq (Get-Service -Name $service -ErrorAction SilentlyContinue)
   $cleanup.Add('SUPERVISOR_SERVICE_DELETE='+$(if($serviceAbsent){'PASS'}else{'FAIL'}))
  }catch{$cleanup.Add('SUPERVISOR_SERVICE_CLEANUP=FAIL')}
 }
 if($badTlsProcess){Stop-Process -Id $badTlsProcess.Id -Force -ErrorAction SilentlyContinue;Wait-Process -Id $badTlsProcess.Id -Timeout 10 -ErrorAction SilentlyContinue}
 $badTlsClosed=Wait-TcpPort '127.0.0.1' 8444 15 $false
 $badTlsAbsent=($null -eq $badTlsProcess -or $null -eq (Get-Process -Id $badTlsProcess.Id -ErrorAction SilentlyContinue))
 $cleanup.Add('BAD_TLS_PROCESS_CLEANUP='+$(if($badTlsClosed -and $badTlsAbsent){'PASS'}else{'FAIL'}))
 if($keycloakProcess){Stop-Keycloak $keycloakProcess $kcHome}
 $keycloakClosed=Wait-TcpPort '127.0.0.1' 8443 30 $false
 $kcRemaining=@(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue|Where-Object {$_.CommandLine -and $kcHome -and $_.CommandLine.Contains($kcHome)})
 $cleanup.Add('KEYCLOAK_PROCESS_CLEANUP='+$(if($keycloakClosed -and $kcRemaining.Count -eq 0){'PASS'}else{'FAIL'}))
 if($pgService){
  try{
   $env:PGPASSWORD=$pgPassword
   & $psql -h 127.0.0.1 -p 5432 -U postgres -d postgres -v ON_ERROR_STOP=1 -c "DROP DATABASE IF EXISTS $pgDatabase WITH (FORCE)"|Out-Null
   if($LASTEXITCODE -ne 0){throw 'DROP_DATABASE_FAILED'}
   $remaining=& $psql -h 127.0.0.1 -p 5432 -U postgres -d postgres -v ON_ERROR_STOP=1 -Atc "SELECT COUNT(*) FROM pg_database WHERE datname='$pgDatabase'" 2>$null
   if($LASTEXITCODE -ne 0 -or [int]$remaining -ne 0){throw 'DATABASE_STILL_PRESENT'}
   $cleanup.Add('POSTGRESQL_DATABASE_DROP=PASS')
  }catch{$cleanup.Add('POSTGRESQL_DATABASE_DROP=FAIL')}
  if($pgInstalled -or $pgStartedByUs){try{Stop-Service -Name $pgService.Name -Force -ErrorAction Stop;Start-Sleep -Seconds 2;$cleanup.Add('POSTGRESQL_SERVICE_STOP=PASS')}catch{$cleanup.Add('POSTGRESQL_SERVICE_STOP=FAIL')}}else{$cleanup.Add('POSTGRESQL_SERVICE_RESTORED=PASS')}
 }
 if($pgInstalled){
  try{
   & choco uninstall postgresql16 -y --no-progress --limit-output|Out-Null
   if($LASTEXITCODE -ne 0){throw 'UNINSTALL_FAILED'}
   $inventory=& choco list --exact postgresql16 --limit-output 2>$null
   $inventoryExit=$LASTEXITCODE
   $packageListed=@($inventory|Where-Object {$_ -match '^postgresql16\|'}).Count -gt 0
   $binaryPresent=Test-Path $psql
   if($inventoryExit -ne 0 -or $packageListed -or $binaryPresent){throw 'POSTGRESQL_PACKAGE_REMAINS'}
   $cleanup.Add('POSTGRESQL_PACKAGE_UNINSTALL=PASS')
   $cleanup.Add('POSTGRESQL_PACKAGE_ABSENT=PASS')
  }catch{$cleanup.Add('POSTGRESQL_PACKAGE_UNINSTALL=FAIL');$cleanup.Add('POSTGRESQL_PACKAGE_ABSENT=FAIL')}
  try{
   $pgData=Join-Path $env:ProgramFiles 'PostgreSQL\\16\\data'
   if(Test-Path $pgData){Remove-Item $pgData -Recurse -Force}
   $cleanup.Add('POSTGRESQL_DATA_DIRECTORY_REMOVED='+$(if(-not(Test-Path $pgData)){'PASS'}else{'FAIL'}))
  }catch{$cleanup.Add('POSTGRESQL_DATA_DIRECTORY_REMOVED=FAIL')}
 }
 try{if($rootThumbprint){Remove-Item "Cert:\LocalMachine\Root\$rootThumbprint" -Force -ErrorAction Stop};$cleanup.Add('TEST_CA_REMOVED=PASS')}catch{$cleanup.Add('TEST_CA_REMOVED=FAIL')}
 try{if($hostsOriginal -ne $null){[IO.File]::WriteAllText($hostsPath,$hostsOriginal);$restored=[IO.File]::ReadAllText($hostsPath);if($restored -cne $hostsOriginal){throw 'HOSTS_RESTORE_MISMATCH'}};$cleanup.Add('HOSTS_FILE_RESTORED=PASS')}catch{$cleanup.Add('HOSTS_FILE_RESTORED=FAIL')}
 try{
  if(Test-Path $storeRoot){Remove-Item $storeRoot -Recurse -Force}
  $phone02Parent=Split-Path $storeRoot -Parent
  if((Test-Path $phone02Parent) -and @(Get-ChildItem $phone02Parent -Force).Count -eq 0){Remove-Item $phone02Parent -Force}
  $tokenSaverParent=Split-Path $phone02Parent -Parent
  if((Test-Path $tokenSaverParent) -and @(Get-ChildItem $tokenSaverParent -Force).Count -eq 0){Remove-Item $tokenSaverParent -Force}
  $cleanup.Add('DPAPI_STORE_DIRECTORY_REMOVED='+$(if(-not(Test-Path $storeRoot)){'PASS'}else{'FAIL'}))
 }catch{$cleanup.Add('DPAPI_STORE_DIRECTORY_REMOVED=FAIL')}
 Remove-Item Env:PGPASSWORD,Env:KC_BOOTSTRAP_ADMIN_USERNAME,Env:KC_BOOTSTRAP_ADMIN_PASSWORD,Env:KC_DB_PASSWORD,Env:KC_DB_URL_HOST,Env:KC_DB_URL_PORT,Env:KC_DB_URL_DATABASE,Env:KC_DB_USERNAME,Env:KC_DB,Env:KC_HTTP_ENABLED,Env:KC_HTTPS_CERTIFICATE_FILE,Env:KC_HTTPS_CERTIFICATE_KEY_FILE,Env:KC_HTTPS_PORT,Env:KC_HOSTNAME,Env:KC_HOSTNAME_STRICT -ErrorAction SilentlyContinue
 $adminToken=$null;$adminResponse=$null;$secretInfo=$null;$oidcClientSecret=$null;$adminPassword=$null;$pgPassword=$null
 $serviceBody=if($svcLog -and (Test-Path $svcLog)){Get-Content $svcLog -Raw}else{''}
 $required=@('CREDENTIAL_STORE_INTEGRATED=PASS','LOCAL_SERVICE=PASS','SERVICE_SID=PASS','CREDENTIAL_PROVISIONED=PASS','CREDENTIAL_PROVISIONING_ADMIN_ONLY=PASS','CREDENTIAL_STORE_READ=PASS','CREDENTIAL_PROTECTED=PASS','DPAPI_NG_SERVICE_SID_BOUND=PASS','OIDC_SECRET_BUFFER_ZEROED=PASS','KEYCLOAK_HTTPS=PASS','OIDC_DISCOVERY=PASS','OIDC_ISSUER_VALID=PASS','CLIENT_CREDENTIALS=PASS','TOKEN_ACQUIRED=PASS','TOKEN_SIGNATURE=PASS','TOKEN_ISSUER=PASS','TOKEN_AUDIENCE=PASS','TOKEN_SCOPE=PASS','TOKEN_EXPIRY=PASS','TOKEN_MEMORY_ONLY=PASS','NEGATIVE_WRONG_SECRET=PASS','NEGATIVE_WRONG_CLIENT=PASS','NEGATIVE_INVALID_SCOPE=PASS','NEGATIVE_BAD_ENDPOINT=PASS','NEGATIVE_TLS=PASS','NEGATIVE_KEYCLOAK_UNAVAILABLE=PASS','NEGATIVE_MALFORMED_RESPONSE=PASS','SUPERVISOR_OIDC_TOKEN_EXCHANGE=PASS','RPC_AUTH_EXCHANGE_CALLED=NO','TOKEN_MEMORY_ZEROED=PASS','CREDENTIAL_REMOVE=PASS','CLEANUP=PASS','R5_VERDICT=PASS')
 $allServiceMarkers=$true
 foreach($marker in $required){if(-not $serviceBody.Contains($marker)){$allServiceMarkers=$false}}
 if($svcLog -and (Test-Path $svcLog)){Copy-Item $svcLog (Join-Path $evidence 'supervisor-r5.log') -Force}
 if($clientLog -and (Test-Path $clientLog)){Copy-Item $clientLog (Join-Path $evidence 'supervisor-client-common.log') -Force}
 try{
  if(Test-Path $clientLog){Remove-Item $clientLog -Force}
  if(Test-Path $root){
   for($attempt=1;$attempt -le 10 -and (Test-Path $root);$attempt++){
    $remainingRootProcesses=@(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue|Where-Object {$_.CommandLine -and $_.CommandLine.Contains($root)})
    foreach($rp in $remainingRootProcesses){Stop-Process -Id $rp.ProcessId -Force -ErrorAction SilentlyContinue}
    Start-Sleep -Seconds 3
    try{Remove-Item $root -Recurse -Force -ErrorAction Stop}catch{
     $errText=[string]$_.Exception.Message
     $errText=$errText -replace '(?i)(password|secret|token|credential)\s*[:=]\s*[^\s,;]+','$1=[REDACTED]'
     Write-Host ('TEMP_DIRECTORY_REMOVE_ATTEMPT='+$attempt+';ERROR='+$errText)
     $lockedPath=$_.TargetObject
     if($lockedPath){Write-Host ('TEMP_DIRECTORY_LOCKED_TARGET='+$lockedPath)}
     $remainingFiles=@(Get-ChildItem $root -File -Recurse -Force -ErrorAction SilentlyContinue|Select-Object -First 15 -ExpandProperty FullName)
     foreach($remain in $remainingFiles){Write-Host ('TEMP_DIRECTORY_REMAINING_FILE='+$remain)}
     $rootProcesses=@(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue|Where-Object {$_.CommandLine -and $_.CommandLine.Contains($root)})
     foreach($rp in $rootProcesses){Write-Host ('TEMP_DIRECTORY_REMAINING_PROCESS='+$rp.ProcessId+':'+$rp.Name)}
    }
   }
  }
  $cleanup.Add('TEMP_DIRECTORY_REMOVED='+$(if(-not(Test-Path $root)){'PASS'}else{'FAIL'}))
 }catch{$cleanup.Add('TEMP_DIRECTORY_REMOVED=FAIL')}
 $cleanupResult=if($cleanup -match '=FAIL'){'FAIL'}else{'PASS'}
 $cleanup.Add('CLEANUP='+$cleanupResult)
 $final=($null -eq $failure -and $cleanupResult -eq 'PASS' -and $allServiceMarkers -and $checks -contains 'KEYCLOAK_CLIENT_CONFIDENTIAL=PASS' -and $checks -contains 'KEYCLOAK_SERVICE_ACCOUNT_ENABLED=PASS' -and $checks -contains 'KEYCLOAK_SCOPE_AND_AUDIENCE_CONFIGURED=PASS' -and $checks -contains 'KEYCLOAK_POSTGRES_REALM_PERSISTED=PASS' -and $checks -contains 'OIDC_JWKS_RSA_PRESENT=PASS' -and $checks -contains 'KEYCLOAK_HTTP_DISABLED=PASS' -and $checks -contains 'KEYCLOAK_STOPPED_FOR_NEGATIVE_PROBES=PASS' -and $checks -contains 'BAD_TLS_PROBE_LISTENER_READY=PASS' -and $checks -contains 'SUPERVISOR_OIDC_R5_VERDICT=PASS' -and $checks -contains 'RPC_TOKEN_EXPOSURE=PASS' -and $checks -contains 'COMMON_PROCESS_REJECT=PASS' -and $checks -contains 'COMMON_PROCESS_TEAM_ROOM_REJECT=PASS' -and $checks -contains 'CLIENT_SECRET_NOT_IN_CLI_ENV_FILE_RPC=PASS' -and $checks -contains 'TOKEN_LOG_EXPOSURE=PASS')
 $summary=@(
  'branch='+$env:GITHUB_REF
  'sha='+$env:GITHUB_SHA
  'run_id='+$env:GITHUB_RUN_ID
  'R4_APP_CONTAINER_EVIDENCE=PASS'
  'R4_RUN_ID=44188631595'
  'R4_ARTIFACT_ID=14374149971'
  'R4_ARTIFACT_SHA256=69e317dd3b7d280ecef5fef2eed8f2d8308c11af211a0ed017a74d5912c639ea'
  'SECRET_SCAN='+$(if($checks -contains 'CLIENT_SECRET_NOT_IN_CLI_ENV_FILE_RPC=PASS' -and $checks -contains 'TOKEN_LOG_EXPOSURE=PASS'){'PASS'}else{'FAIL'})
  'ARTIFACT_SCAN=PENDING_WORKFLOW'
  'CLEANUP='+$cleanupResult
  'R5_RUNTIME_VERDICT='+$(if($final){'PASS'}else{'BLOCKED'})
  'checks='+($checks -join ';')
  'cleanup='+($cleanup -join ';')
 )
 $summary|Set-Content (Join-Path $evidence 'r5-summary.txt') -Encoding utf8
 $hashes=Get-ChildItem $evidence -File|Where-Object Name -ne 'SHA256SUMS'|ForEach-Object {(Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()+'  '+$_.Name}
 $hashes|Set-Content (Join-Path $evidence 'SHA256SUMS') -Encoding ascii
 if($null -ne $failure -or -not $final){Write-Host 'R5_RUNTIME_FINAL=FAIL'}else{Write-Host 'R5_RUNTIME_FINAL=PASS'}
}
if($null -ne $failure -or -not $final){throw 'R5_RUNTIME_VERDICT_BLOCKED'}
Write-Host 'R5_RUNTIME_VERDICT=PASS'
