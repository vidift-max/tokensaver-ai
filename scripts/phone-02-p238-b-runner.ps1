$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = Join-Path $env:RUNNER_TEMP 'phone02-p238b-runner'
$evidence = Join-Path $env:RUNNER_TEMP 'phone02-p238b-evidence'
$transcript = Join-Path $evidence 'runner-transcript.log'
$pgPassword = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
$pgInstalled = $false; $pgService = $null; $keycloakProcess = $null; $teamRoomProcess = $null; $failure = $null
$cleanup = [System.Collections.Generic.List[string]]::new()
New-Item -ItemType Directory -Force -Path $root,$evidence | Out-Null
Start-Transcript -Path $transcript -Force | Out-Null
function Assert-Check([bool]$Condition,[string]$Name) {
    Write-Host ($Name + '=' + $(if($Condition){'PASS'}else{'FAIL'}))
    if(-not $Condition){throw ($Name + ' failed')}
}
function Wait-Http([string]$Uri,[int]$Seconds,[string]$Name) {
    $deadline=(Get-Date).AddSeconds($Seconds)
    do {
        try { $response=Invoke-WebRequest -Uri $Uri -UseBasicParsing -TimeoutSec 3; if($response.StatusCode -eq 200){Write-Host ($Name+'=READY');return} } catch {}
        Start-Sleep -Seconds 2
    } while((Get-Date) -lt $deadline)
    throw ($Name+' did not become ready: '+$Uri)
}
function Stop-TrackedProcess($Process,[string]$Name) {
    if($null -ne $Process) {
        $p=Get-Process -Id $Process.Id -ErrorAction SilentlyContinue
        if($p){Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue;Wait-Process -Id $Process.Id -Timeout 10 -ErrorAction SilentlyContinue}
        $alive=[bool](Get-Process -Id $Process.Id -ErrorAction SilentlyContinue)
        $cleanup.Add($Name+'='+$(if($alive){'FAIL'}else{'PASS'}))
    } else {$cleanup.Add($Name+'=NOT_STARTED')}
}
try {
    Assert-Check ($env:GITHUB_REF -eq 'refs/heads/mission/phone-02-p238-b-oidc') 'EXPLICIT_BRANCH_GUARD'
    Assert-Check ($env:RUNNER_OS -eq 'Windows' -and $env:RUNNER_ARCH -eq 'X64') 'WINDOWS_X64_RUNNER'
    $programFilesX86=[Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
    Assert-Check (Test-Path (Join-Path $programFilesX86 'Microsoft Visual Studio\Installer\vswhere.exe')) 'VISUAL_STUDIO_BUILD_TOOLS'
    Assert-Check ([bool](Get-Command cargo -ErrorAction SilentlyContinue)) 'RUST_TOOLCHAIN'
    Assert-Check ([bool](Get-Command java -ErrorAction SilentlyContinue)) 'JAVA_TOOLCHAIN'
    Write-Host ('RUN_ID='+$env:GITHUB_RUN_ID)
    Write-Host ('RUN_ATTEMPT='+$env:GITHUB_RUN_ATTEMPT)
    Write-Host ('COMMIT_SHA='+$env:GITHUB_SHA)
    Write-Host ('BRANCH='+$env:GITHUB_REF)
    Write-Host ('RUNNER_NAME='+$env:RUNNER_NAME)
    Write-Host ('RUNNER_IMAGE_OS='+$env:ImageOS)
    Write-Host ('RUNNER_IMAGE_VERSION='+$env:ImageVersion)

    # Native Windows PostgreSQL: Windows jobs cannot use Linux service containers.
    $chocoArgs=@('install','postgresql16','--version=16.15.0','--params',("/Password:$pgPassword /Port:5432 /NoPath"),'--yes','--no-progress','--limit-output')
    & choco @chocoArgs
    if($LASTEXITCODE -ne 0){throw ('Chocolatey PostgreSQL install failed: '+$LASTEXITCODE)}
    $pgInstalled=$true
    $pgService=Get-Service | Where-Object {$_.Name -match '^postgresql.*16'} | Select-Object -First 1
    if(-not $pgService){throw 'PostgreSQL 16 Windows service not found after install'}
    if($pgService.Status -ne 'Running'){Start-Service -Name $pgService.Name}
    $psql = Join-Path $env:ProgramFiles 'PostgreSQL\16\bin\psql.exe'
    Assert-Check (Test-Path $psql) 'POSTGRESQL_CLIENT_PRESENT'
    $deadline=(Get-Date).AddSeconds(60);$pgReady=$false
    do {
        $env:PGPASSWORD=$pgPassword
        & $psql -h 127.0.0.1 -p 5432 -U postgres -d postgres -v ON_ERROR_STOP=1 -Atc 'SELECT 1' 2>$null
        $pgReady=($LASTEXITCODE -eq 0)
        if(-not $pgReady){Start-Sleep -Seconds 2}
    } while(-not $pgReady -and (Get-Date) -lt $deadline)
    Assert-Check $pgReady 'POSTGRESQL_NATIVE_SERVICE_READY'
    & $psql -h 127.0.0.1 -p 5432 -U postgres -d postgres -v ON_ERROR_STOP=1 -c 'CREATE DATABASE team_room_p238b'
    if($LASTEXITCODE -ne 0){throw 'PostgreSQL disposable Team Room database creation failed'}
    & $psql -h 127.0.0.1 -p 5432 -U postgres -d postgres -v ON_ERROR_STOP=1 -c 'CREATE DATABASE keycloak_p238b'
    if($LASTEXITCODE -ne 0){throw 'PostgreSQL disposable Keycloak database creation failed'}
    Write-Host 'POSTGRESQL_DATABASE=team_room_p238b'
    Write-Host 'POSTGRESQL_DATABASE=keycloak_p238b'
    Remove-Item Env:PGPASSWORD -ErrorAction SilentlyContinue

        # P2.38-B local realm import occurs before the first Keycloak start.
    # This phase does not run Phone-02, Supervisor, or MSIX.
    $keycloakZip=Join-Path $root 'keycloak-26.3.5.zip'
    $keycloakHome=Join-Path $root 'keycloak-26.3.5'
    Invoke-WebRequest -Uri 'https://github.com/keycloak/keycloak/releases/download/26.3.5/keycloak-26.3.5.zip' -OutFile $keycloakZip
    $keycloakZipHash=(Get-FileHash $keycloakZip -Algorithm SHA256).Hash
    Expand-Archive -Path $keycloakZip -DestinationPath $root -Force
    $kcBat=Join-Path $keycloakHome 'bin\kc.bat'
    Assert-Check (Test-Path $kcBat) 'KEYCLOAK_DISTRIBUTION_PRESENT'
    Write-Host 'KEYCLOAK_VERSION=26.3.5'
    Write-Host ('KEYCLOAK_ARCHIVE_SHA256='+$keycloakZipHash)
    $env:KC_HEALTH_ENABLED='true';$env:KC_HTTP_ENABLED='true'
    $env:KC_DB='postgres'
    $env:KC_DB_URL='jdbc:postgresql://127.0.0.1:5432/keycloak_p238b?ApplicationName=keycloak-p238b'
    $env:KC_DB_USERNAME='postgres';$env:KC_DB_PASSWORD=$pgPassword
    $env:P238B_KEYCLOAK_PG_PASSWORD=$pgPassword;$env:PGPASSWORD=$pgPassword
    $bootstrapScript=Join-Path $env:GITHUB_WORKSPACE 'scripts\phone-02-p238b-keycloak-bootstrap.ps1'
    Assert-Check (Test-Path $bootstrapScript) 'KEYCLOAK_P238B_BOOTSTRAP_PRESENT'
    $bootstrapResult=& $bootstrapScript -KeycloakBat $kcBat -KeycloakHome $keycloakHome -Evidence $evidence -PsqlPath $psql
    if(-not $bootstrapResult -or -not $bootstrapResult.Process -or -not $bootstrapResult.Secret){throw 'Keycloak bootstrap did not return its in-memory result'}
    $keycloakProcess=$bootstrapResult.Process;$oidcClientSecret=$bootstrapResult.Secret
    Remove-Item Env:PGPASSWORD,Env:KC_HEALTH_ENABLED,Env:KC_HTTP_ENABLED,Env:KC_DB,Env:KC_DB_URL,Env:KC_DB_USERNAME,Env:KC_DB_PASSWORD,Env:P238B_KEYCLOAK_PG_PASSWORD -ErrorAction SilentlyContinue
    Write-Host 'OIDC_ACCESS_TOKEN=NOT_REQUESTED'
    Write-Host 'TEAM_ROOM_AUTH_EXCHANGE=NOT_CALLED'
    Write-Host 'AI_PHONE_ACTIVATION=NOT_PERFORMED'
    Write-Host 'P238B_PHASE1_BOOTSTRAP=CHECKS_COMPLETED_PENDING_CLEANUP'
}
catch {
    $failure=$_
    if($_.Exception.Data.Contains('OIDC_SECRET_IN_MEMORY')){$oidcClientSecret=[string]$_.Exception.Data['OIDC_SECRET_IN_MEMORY']}
    Write-Host ('P238B_RUNNER_FAILURE='+$_.Exception.Message)
}
finally {
    Remove-Item Env:PGPASSWORD,Env:DATABASE_URL,Env:TEAM_ROOM_BIND,Env:TEAM_ROOM_HTTPS_BIND,Env:TEAM_ROOM_TLS_CERT_PATH,Env:TEAM_ROOM_TLS_KEY_PATH,Env:P238_TEAM_ROOM_FULL_MODE,Env:RUST_LOG,Env:KC_BOOTSTRAP_ADMIN_USERNAME,Env:KC_BOOTSTRAP_ADMIN_PASSWORD,Env:KC_HEALTH_ENABLED,Env:KC_HTTP_ENABLED,Env:KC_DB,Env:KC_DB_URL,Env:KC_DB_USERNAME,Env:KC_DB_PASSWORD,Env:P238B_KEYCLOAK_PG_PASSWORD -ErrorAction SilentlyContinue
    $cleanup.Add('PHONE02_SUPERVISOR_MSIX_HARNESS=NOT_RUN_BY_PHASE1')
    $kcDir=Join-Path $root 'keycloak-26.3.5'
    $kcChildren=@(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {$_.CommandLine -and $_.CommandLine.Contains($kcDir)})
    foreach($child in $kcChildren){Stop-Process -Id $child.ProcessId -Force -ErrorAction SilentlyContinue}
    if($kcChildren.Count -gt 0){Start-Sleep -Seconds 2}
    $kcRemaining=@(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {$_.CommandLine -and $_.CommandLine.Contains($kcDir)})
    $cleanup.Add('KEYCLOAK_CHILD_PROCESSES_STOP='+$(if($kcRemaining.Count -eq 0){'PASS'}else{'FAIL'}))
    if($keycloakProcess){Stop-TrackedProcess $keycloakProcess 'KEYCLOAK_LAUNCHER_STOP'}else{$cleanup.Add('KEYCLOAK_LAUNCHER_STOP=BOOTSTRAP_DID_NOT_RETURN')}
    try {Stop-Transcript | Out-Null} catch {$failure=[System.Exception]::new('Transcript shutdown failed before secret sanitization')}
    # Keycloak and transcript writers are stopped before evidence files are read.
    $sanitizationFailed=$false;$sanitizationReason='none'
    try {
        foreach($file in Get-ChildItem $evidence -File -ErrorAction Stop) {
            $raw=[IO.File]::ReadAllText($file.FullName)
            if($null -ne $oidcClientSecret -and $oidcClientSecret.Length -gt 0){$raw=$raw.Replace($oidcClientSecret,'[REDACTED_OIDC_SECRET]')}
            $raw=$raw.Replace($pgPassword,'[REDACTED_PG_PASSWORD]')
            $raw=[regex]::Replace($raw,'eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}','[REDACTED_JWT]')
            [IO.File]::WriteAllText($file.FullName,$raw)
        }
        foreach($file in Get-ChildItem $evidence -File -ErrorAction Stop) {
            $check=[IO.File]::ReadAllText($file.FullName)
            if(($null -ne $oidcClientSecret -and $oidcClientSecret.Length -gt 0 -and $check.Contains($oidcClientSecret)) -or $check.Contains($pgPassword)){throw 'secret material remains in evidence'}
        }
    } catch {
        $sanitizationFailed=$true
        $sanitizationReason='evidence_read_redact_or_verify_failed;type='+$_.Exception.GetType().FullName+';hresult='+('0x{0:X8}' -f $_.Exception.HResult)
    }
    if($sanitizationFailed) {
        Write-Host 'OIDC_SECRET_SANITIZATION=FAIL'
        Write-Host ('OIDC_SECRET_SANITIZATION_REASON='+$sanitizationReason)
        $cleanup.Add('OIDC_SECRET_SANITIZATION=FAIL')
        if($null -eq $failure){$failure=[System.Exception]::new('OIDC secret sanitization failed')}
    } else {Write-Host 'OIDC_SECRET_SANITIZATION=PASS';$cleanup.Add('OIDC_SECRET_SANITIZATION=PASS')}
    $oidcClientSecret=$null;$bootstrapResult=$null
    if($failure -and $failure.Exception.Data.Contains('OIDC_SECRET_IN_MEMORY')){$failure.Exception.Data.Remove('OIDC_SECRET_IN_MEMORY')}
    if($failure -and $failure.Exception.Data.Contains('ORIGINAL_EXCEPTION')){$failure.Exception.Data.Remove('ORIGINAL_EXCEPTION')}
    try {Remove-Item $root -Recurse -Force -ErrorAction Stop;$cleanup.Add('EPHEMERAL_DOWNLOAD_DIRECTORY_REMOVED=PASS')} catch {$cleanup.Add('EPHEMERAL_DOWNLOAD_DIRECTORY_REMOVED=FAIL')}
    if(Test-Path $root){$cleanup.Add('EPHEMERAL_DOWNLOAD_DIRECTORY_VERIFY=FAIL')}else{$cleanup.Add('EPHEMERAL_DOWNLOAD_DIRECTORY_VERIFY=PASS')}
    if($pgService) {
        try {
            $env:PGPASSWORD=$pgPassword
            & $psql -h 127.0.0.1 -p 5432 -U postgres -d postgres -v ON_ERROR_STOP=1 -c 'DROP DATABASE IF EXISTS keycloak_p238b WITH (FORCE)' | Out-Null
            if($LASTEXITCODE -ne 0){throw 'Keycloak database cleanup failed'}
            & $psql -h 127.0.0.1 -p 5432 -U postgres -d postgres -v ON_ERROR_STOP=1 -c 'DROP DATABASE IF EXISTS team_room_p238b WITH (FORCE)' | Out-Null
            if($LASTEXITCODE -ne 0){throw 'Team Room database cleanup failed'}
            $cleanup.Add('POSTGRESQL_DATABASES_DROP=PASS')
        } catch {$cleanup.Add('POSTGRESQL_DATABASES_DROP=FAIL')}
        # Verify database absence while PostgreSQL is still reachable.
        try {
            $dbCountText=& $psql -h 127.0.0.1 -p 5432 -U postgres -d postgres -v ON_ERROR_STOP=1 -Atc "SELECT COUNT(*) FROM pg_database WHERE datname IN ('team_room_p238b','keycloak_p238b')" 2>$null
            $dbQueryOk=($LASTEXITCODE -eq 0)
            $dbCount=-1
            $dbParsed=[int]::TryParse((($dbCountText | Out-String).Trim()),[ref]$dbCount)
            $dbAbsent=($dbQueryOk -and $dbParsed -and $dbCount -eq 0)
            Write-Host ('POSTGRESQL_DATABASES_ABSENT='+$(if($dbAbsent){'PASS'}else{'FAIL'}))
            $cleanup.Add('POSTGRESQL_DATABASES_ABSENT='+$(if($dbAbsent){'PASS'}else{'FAIL'}))
            if(-not $dbAbsent -and $null -eq $failure){$failure=[System.Exception]::new('PostgreSQL disposable database absence verification failed')}
        } catch {
            Write-Host 'POSTGRESQL_DATABASES_ABSENT=FAIL';$cleanup.Add('POSTGRESQL_DATABASES_ABSENT=FAIL')
            if($null -eq $failure){$failure=[System.Exception]::new('PostgreSQL database absence verification failed')}
        }
        try {
            Stop-Service -Name $pgService.Name -Force -ErrorAction Stop
            $serviceDeadline=(Get-Date).AddSeconds(30)
            do {
                $serviceAfterStop=Get-Service -Name $pgService.Name -ErrorAction Stop
                if($serviceAfterStop.Status -eq [System.ServiceProcess.ServiceControllerStatus]::Stopped){break}
                Start-Sleep -Seconds 1
            } while((Get-Date) -lt $serviceDeadline)
            if($serviceAfterStop.Status -ne [System.ServiceProcess.ServiceControllerStatus]::Stopped){throw 'PostgreSQL service remains active'}
            $cleanup.Add('POSTGRESQL_SERVICE_STOP=PASS')
        } catch {$cleanup.Add('POSTGRESQL_SERVICE_STOP=FAIL')}
        try {
            # Independent state read after Stop-Service and its bounded wait.
            $serviceVerify=Get-Service -Name $pgService.Name -ErrorAction Stop
            $serviceStopped=($serviceVerify.Status -eq [System.ServiceProcess.ServiceControllerStatus]::Stopped)
            Write-Host ('POSTGRESQL_SERVICE_STOPPED_VERIFY='+$(if($serviceStopped){'PASS'}else{'FAIL'}))
            $cleanup.Add('POSTGRESQL_SERVICE_STOPPED_VERIFY='+$(if($serviceStopped){'PASS'}else{'FAIL'}))
            if(-not $serviceStopped -and $null -eq $failure){$failure=[System.Exception]::new('PostgreSQL service post-stop verification failed')}
        } catch {
            Write-Host 'POSTGRESQL_SERVICE_STOPPED_VERIFY=FAIL';$cleanup.Add('POSTGRESQL_SERVICE_STOPPED_VERIFY=FAIL')
            if($null -eq $failure){$failure=[System.Exception]::new('PostgreSQL service state could not be verified')}
        }
    } else {
        Write-Host 'POSTGRESQL_DATABASES_ABSENT=FAIL';$cleanup.Add('POSTGRESQL_DATABASES_ABSENT=FAIL')
        Write-Host 'POSTGRESQL_SERVICE_STOPPED_VERIFY=FAIL';$cleanup.Add('POSTGRESQL_SERVICE_STOPPED_VERIFY=FAIL')
        $cleanup.Add('POSTGRESQL_SERVICE_STOP=NOT_STARTED')
        if($null -eq $failure){$failure=[System.Exception]::new('PostgreSQL service was not provisioned; cleanup postconditions cannot pass')}
    }
    if($pgInstalled) {
        try {
            & choco uninstall postgresql16 -y --no-progress --limit-output | Out-Null
            if($LASTEXITCODE -ne 0){throw 'Chocolatey uninstall returned non-zero'}
            $cleanup.Add('POSTGRESQL_PACKAGE_UNINSTALL=PASS')
        } catch {$cleanup.Add('POSTGRESQL_PACKAGE_UNINSTALL=FAIL')}
        try {
            $inventory=& choco list --exact postgresql16 --limit-output 2>$null
            $inventoryExit=$LASTEXITCODE
            $packageListed=@($inventory | Where-Object {$_ -match '^postgresql16\|'}).Count -gt 0
            $binaryPresent=Test-Path $psql
            $packageAbsent=($inventoryExit -eq 0 -and -not $packageListed -and -not $binaryPresent)
            Write-Host ('POSTGRESQL_PACKAGE_ABSENT='+$(if($packageAbsent){'PASS'}else{'FAIL'}))
            $cleanup.Add('POSTGRESQL_PACKAGE_ABSENT='+$(if($packageAbsent){'PASS'}else{'FAIL'}))
            if(-not $packageAbsent -and $null -eq $failure){$failure=[System.Exception]::new('PostgreSQL package or installation binary remains after uninstall')}
        } catch {
            Write-Host 'POSTGRESQL_PACKAGE_ABSENT=FAIL';$cleanup.Add('POSTGRESQL_PACKAGE_ABSENT=FAIL')
            if($null -eq $failure){$failure=[System.Exception]::new('PostgreSQL package absence verification failed')}
        }
    } else {
        $cleanup.Add('POSTGRESQL_PACKAGE_UNINSTALL=NOT_INSTALLED')
        Write-Host 'POSTGRESQL_PACKAGE_ABSENT=FAIL';$cleanup.Add('POSTGRESQL_PACKAGE_ABSENT=FAIL')
        if($null -eq $failure){$failure=[System.Exception]::new('PostgreSQL package was not installed; expected runner package cannot be verified absent')}
    }
    Remove-Item Env:PGPASSWORD -ErrorAction SilentlyContinue
    if($cleanup -match '=FAIL'){$cleanupResult='FAIL';if($null -eq $failure){$failure=[System.Exception]::new('One or more ephemeral cleanup checks failed')}}else{$cleanupResult='PASS'}
    Write-Host ('CLEANUP='+$cleanupResult)
    @(
        'branch='+$env:GITHUB_REF
        'sha='+$env:GITHUB_SHA
        'run_id='+$env:GITHUB_RUN_ID
        'run_attempt='+$env:GITHUB_RUN_ATTEMPT
        'runner_os='+$env:RUNNER_OS
        'runner_name='+$env:RUNNER_NAME
        'result='+$(if($null -eq $failure){'COMPLETED'}else{'FAIL'})
        'oidc_flow=NOT_EXECUTED'
        'cleanup='+($cleanup -join ';')
    ) | Set-Content (Join-Path $evidence 'runner-metadata.txt') -Encoding utf8
    $hashLines=Get-ChildItem $evidence -File | Where-Object Name -ne 'SHA256SUMS' | ForEach-Object {(Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()+'  '+$_.Name}
    $hashLines | Set-Content (Join-Path $evidence 'SHA256SUMS') -Encoding ascii
}
if($null -ne $failure){throw $failure}
Write-Host 'P238B_RUNNER_BOOTSTRAP=COMPLETED'
