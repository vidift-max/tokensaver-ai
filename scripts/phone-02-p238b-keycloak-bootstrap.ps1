param([string]$KeycloakBat,[string]$KeycloakHome,[string]$Evidence,[string]$PsqlPath)
$ErrorActionPreference='Stop'
$realmName='phone02-p238b';$clientId='phone02-supervisor-p238b';$scopeName='team_room:connect';$audience='team-room-api'
$issuer="http://127.0.0.1:8080/realms/$realmName"
$importDir=Join-Path $KeycloakHome 'data/import';$importFile=Join-Path $importDir 'phone02-p238b-realm.json'
$secret=$null;$secretBytes=$null;$process=$null
function Mark([string]$name,[bool]$ok){Write-Host ($name+'='+$(if($ok){'PASS'}else{'FAIL'}));if(-not $ok){throw ($name+' failed')}}
function Sql([string]$db,[string]$query){$v=& $PsqlPath -h 127.0.0.1 -p 5432 -U postgres -d $db -v ON_ERROR_STOP=1 -Atc $query 2>$null;if($LASTEXITCODE -ne 0){throw 'PostgreSQL verification query failed'};return (($v|Out-String).Trim())}
try {
 if(!(Test-Path $KeycloakBat) -or !(Test-Path $PsqlPath) -or !$env:P238B_KEYCLOAK_PG_PASSWORD){throw 'Bootstrap prerequisite missing'}
 New-Item -ItemType Directory -Force -Path $importDir|Out-Null
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent().User
 $dacl=New-Object Security.AccessControl.DirectorySecurity;$dacl.SetAccessRuleProtection($true,$false)
 [void]$dacl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($identity,'FullControl','ContainerInherit,ObjectInherit','None','Allow'))
 Set-Acl -LiteralPath $importDir -AclObject $dacl
 $secretBytes=New-Object byte[] 32;[Security.Cryptography.RandomNumberGenerator]::Fill($secretBytes)
 $secret=[Convert]::ToBase64String($secretBytes).TrimEnd('=').Replace('+','-').Replace('/','_')
 $realm=[ordered]@{
  realm=$realmName;enabled=$true;displayName='P2.38-B isolated OIDC validation realm';sslRequired='none';defaultSignatureAlgorithm='RS256'
  clientScopes=@(@{name=$scopeName;description='P2.38-B Team Room connection scope';protocol='openid-connect';attributes=@{'include.in.token.scope'='true';'display.on.consent.screen'='false'};protocolMappers=@(@{name='team-room-api-audience';protocol='openid-connect';protocolMapper='oidc-audience-mapper';config=@{'included.client.audience'=$audience;'id.token.claim'='false';'access.token.claim'='true'}})})
  clients=@(@{clientId=$clientId;name='P2.38-B Windows Supervisor test client';enabled=$true;protocol='openid-connect';publicClient=$false;bearerOnly=$false;clientAuthenticatorType='client-secret';secret=$secret;serviceAccountsEnabled=$true;standardFlowEnabled=$false;implicitFlowEnabled=$false;directAccessGrantsEnabled=$false;authorizationServicesEnabled=$false;defaultClientScopes=@($scopeName,'profile','email','roles','web-origins');optionalClientScopes=@();attributes=@{'oauth2.device.authorization.grant.enabled'='false';'oidc.ciba.grant.enabled'='false'}})
 }
 $json=$realm|ConvertTo-Json -Depth 30;Set-Content -LiteralPath $importFile -Value $json -Encoding utf8
 $facl=Get-Acl -LiteralPath $importFile;$facl.SetAccessRuleProtection($true,$false)
 [void]$facl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($identity,'FullControl','Allow'))
 Set-Acl -LiteralPath $importFile -AclObject $facl;$json=$null;$realm=$null
 $process=Start-Process -FilePath $KeycloakBat -ArgumentList 'start-dev --db=postgres --http-enabled=true --hostname-strict=false --import-realm' -WorkingDirectory $KeycloakHome -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $Evidence 'keycloak.stdout.log') -RedirectStandardError (Join-Path $Evidence 'keycloak.stderr.log')
 $until=(Get-Date).AddSeconds(240);$ready=$false
 do {if($process.HasExited){throw 'Keycloak exited before ready'};try{$h=Invoke-WebRequest 'http://127.0.0.1:9000/health/ready' -UseBasicParsing -TimeoutSec 3;if($h.StatusCode -eq 200){$ready=$true}}catch{};if(!$ready){Start-Sleep 2}}while(!$ready -and (Get-Date)-lt $until)
 if(!$ready){throw 'Keycloak readiness timeout'}
 $d=Invoke-RestMethod "$issuer/.well-known/openid-configuration" -TimeoutSec 15
 Mark 'KEYCLOAK_TEST_REALM' ($d.issuer -eq $issuer)
 Mark 'KEYCLOAK_DISCOVERY' ($d.issuer -ceq $issuer -and $d.jwks_uri -and $d.token_endpoint)
 $keys=Invoke-RestMethod $d.jwks_uri -TimeoutSec 15
 Mark 'KEYCLOAK_JWKS' (@($keys.keys|Where-Object {$_.kty -eq 'RSA' -and $_.use -eq 'sig' -and $_.kid}).Count -gt 0)
 Mark 'KEYCLOAK_ISSUER_EXACT' ($d.issuer -ceq $issuer)
 $rc=Sql 'keycloak_p238b' "SELECT COUNT(*) FROM realm WHERE name='$realmName'";Mark 'KEYCLOAK_REALM_PERSISTED' ([int]$rc -eq 1)
 $cc=Sql 'keycloak_p238b' "SELECT COUNT(*) FROM client WHERE client_id='$clientId' AND enabled=true AND public_client=false AND client_authenticator_type='client-secret'";Mark 'KEYCLOAK_CLIENT_CONFIDENTIAL' ([int]$cc -eq 1)
 $sc=Sql 'keycloak_p238b' "SELECT COUNT(*) FROM client WHERE client_id='$clientId' AND service_accounts_enabled=true";Mark 'KEYCLOAK_SERVICE_ACCOUNT' ([int]$sc -eq 1)
 Mark 'KEYCLOAK_CLIENT_CREDENTIALS_ENABLED' ([int]$sc -eq 1 -and [int]$cc -eq 1)
 $scope=Sql 'keycloak_p238b' "SELECT COUNT(*) FROM client_scope WHERE name='$scopeName' AND protocol='openid-connect'";Mark 'KEYCLOAK_SCOPE' ([int]$scope -eq 1)
 $assigned=Sql 'keycloak_p238b' "SELECT COUNT(*) FROM client_scope_client d JOIN client c ON c.id=d.client_id JOIN client_scope s ON s.id=d.scope_id WHERE c.client_id='$clientId' AND s.name='$scopeName' AND d.default_scope=true";Mark 'KEYCLOAK_SCOPE_ASSIGNED' ([int]$assigned -eq 1)
 $aud=Sql 'keycloak_p238b' "SELECT COUNT(*) FROM protocol_mapper m JOIN client_scope s ON s.id=m.client_scope_id JOIN protocol_mapper_config c ON c.protocol_mapper_id=m.id WHERE s.name='$scopeName' AND m.protocol_mapper_name='oidc-audience-mapper' AND c.name='included.client.audience' AND c.value='$audience'";Mark 'KEYCLOAK_AUDIENCE' ([int]$aud -gt 0)
 $active=Sql 'postgres' "SELECT COUNT(*) FROM pg_stat_activity WHERE datname='keycloak_p238b' AND usename='postgres'";Mark 'KEYCLOAK_POSTGRES_ACTIVE_CONNECTION' ([int]$active -gt 0)
 $tables=Sql 'keycloak_p238b' "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema='public' AND table_type='BASE TABLE'";Mark 'KEYCLOAK_POSTGRES_SCHEMA_PERSISTED' ([int]$tables -gt 10)
 Write-Host 'KEYCLOAK_EFFECTIVE_DATASTORE=postgresql'
 Remove-Item -LiteralPath $importFile -Force;Mark 'OIDC_IMPORT_SECRET_FILE_REMOVED' (!(Test-Path $importFile))
 # Stop Keycloak before returning so stdout/stderr handles are released.
 $kcChildren=@(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object {$_.CommandLine -and $_.CommandLine.Contains($KeycloakHome)})
 foreach($child in $kcChildren){Stop-Process -Id $child.ProcessId -Force -ErrorAction SilentlyContinue}
 if($process -and !$process.HasExited){Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue}
 if($process){Wait-Process -InputObject $process -Timeout 15 -ErrorAction Stop}
 $stopDeadline=(Get-Date).AddSeconds(20)
 do {$remaining=@(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object {$_.CommandLine -and $_.CommandLine.Contains($KeycloakHome)});if($remaining.Count -eq 0){break};Start-Sleep -Seconds 1} while((Get-Date) -lt $stopDeadline)
 if($remaining.Count -gt 0){throw 'Keycloak process tree did not terminate'}
 Write-Host 'KEYCLOAK_PROCESS_TREE_STOP=PASS'
 Write-Host 'OIDC_ACCESS_TOKEN=NOT_REQUESTED';Write-Host 'OIDC_TOKEN_ENDPOINT=NOT_CALLED';Write-Host 'TEAM_ROOM_AUTH_EXCHANGE=NOT_CALLED'
 return [pscustomobject]@{Process=$process;Secret=$secret}
} catch {
 if(Test-Path $importFile){Remove-Item $importFile -Force -ErrorAction SilentlyContinue}
 # kc.bat may have spawned a Java child that outlives the launcher. Stop the
 # entire isolated Keycloak process tree before the runner hashes evidence logs.
 $kcChildren=@(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {$_.CommandLine -and $_.CommandLine.Contains($KeycloakHome)})
 foreach($child in $kcChildren){Stop-Process -Id $child.ProcessId -Force -ErrorAction SilentlyContinue}
 if($process -and !$process.HasExited){Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue;Wait-Process -Id $process.Id -Timeout 10 -ErrorAction SilentlyContinue}
 $stopDeadline=(Get-Date).AddSeconds(15)
 do {
  $remaining=@(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {$_.CommandLine -and $_.CommandLine.Contains($KeycloakHome)})
  if($remaining.Count -eq 0){break}
  Start-Sleep -Seconds 1
 } while((Get-Date) -lt $stopDeadline)
 if($remaining.Count -gt 0){Write-Host 'KEYCLOAK_FAILURE_PROCESS_CLEANUP=FAIL'}else{Write-Host 'KEYCLOAK_FAILURE_PROCESS_CLEANUP=PASS'}
 $safeType=$_.Exception.GetType().FullName;$safeHResult=('0x{0:X8}' -f $_.Exception.HResult)
 Write-Host ('P238B_BOOTSTRAP_EXCEPTION_TYPE='+$safeType);Write-Host ('P238B_BOOTSTRAP_EXCEPTION_HRESULT='+$safeHResult)
 $safeException=[System.Exception]::new('P2.38-B Keycloak bootstrap failed [type='+$safeType+';hresult='+$safeHResult+']')
 $safeException.Data['ORIGINAL_EXCEPTION']=$_.Exception
 if($null -ne $secret){$safeException.Data['OIDC_SECRET_IN_MEMORY']=$secret}
 throw $safeException
} finally {$secret=$null;if($secretBytes){[Array]::Clear($secretBytes,0,$secretBytes.Length)};Remove-Item Env:P238B_KEYCLOAK_PG_PASSWORD -ErrorAction SilentlyContinue}
