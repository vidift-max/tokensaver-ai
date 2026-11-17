$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = Join-Path $env:RUNNER_TEMP 'phone02-p238b-runner'
$evidence = Join-Path $env:RUNNER_TEMP 'phone02-p238b-evidence'
$transcript = Join-Path $evidence 'runner-transcript.log'
$pgPassword = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
$kcPassword = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
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
    Write-Host 'POSTGRESQL_DATABASE=team_room_p238b'
    Remove-Item Env:PGPASSWORD -ErrorAction SilentlyContinue

    # Official pinned Keycloak ZIP. This round starts the service only; no OIDC
    # client, realm, token acquisition, or token exchange is executed.
    $keycloakZip=Join-Path $root 'keycloak-26.3.5.zip'
    $keycloakHome=Join-Path $root 'keycloak-26.3.5'
    Invoke-WebRequest -Uri 'https://github.com/keycloak/keycloak/releases/download/26.3.5/keycloak-26.3.5.zip' -OutFile $keycloakZip
    $keycloakZipHash=(Get-FileHash $keycloakZip -Algorithm SHA256).Hash
    Expand-Archive -Path $keycloakZip -DestinationPath $root -Force
    $kcBat=Join-Path $keycloakHome 'bin\kc.bat'
    Assert-Check (Test-Path $kcBat) 'KEYCLOAK_DISTRIBUTION_PRESENT'
    Write-Host 'KEYCLOAK_VERSION=26.3.5'
    Write-Host ('KEYCLOAK_ARCHIVE_SHA256='+$keycloakZipHash)
    $env:KC_BOOTSTRAP_ADMIN_USERNAME='p238b-ephemeral-admin'
    $env:KC_BOOTSTRAP_ADMIN_PASSWORD=$kcPassword
    $env:KC_HEALTH_ENABLED='true';$env:KC_HTTP_ENABLED='true'
    $keycloakProcess=Start-Process -FilePath $kcBat -ArgumentList 'start-dev --http-enabled=true --hostname-strict=false' -WorkingDirectory $keycloakHome -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $evidence 'keycloak.stdout.log') -RedirectStandardError (Join-Path $evidence 'keycloak.stderr.log')
    Remove-Item Env:KC_BOOTSTRAP_ADMIN_USERNAME,Env:KC_BOOTSTRAP_ADMIN_PASSWORD,Env:KC_HEALTH_ENABLED,Env:KC_HTTP_ENABLED -ErrorAction SilentlyContinue
    Wait-Http 'http://127.0.0.1:9000/health/ready' 180 'KEYCLOAK_HEALTH'
    Wait-Http 'http://127.0.0.1:8080/realms/master/.well-known/openid-configuration' 30 'KEYCLOAK_DISCOVERY'
    Write-Host ('KEYCLOAK_PROCESS_ID='+$keycloakProcess.Id)

    # Full PostgreSQL-backed Team Room in normal mode on 8082. OIDC remains
    # disabled. P2.38-A's approved script independently exercises the exact
    # Supervisor WinHTTP TLS/AppContainer/MSIX harness against its TLS probe.
    $teamRoomManifest=Join-Path $env:GITHUB_WORKSPACE 'services\ai-team-room\Cargo.toml'
    & cargo build --release --manifest-path $teamRoomManifest --bin ai-team-room-service
    if($LASTEXITCODE -ne 0){throw 'Full Team Room Windows build failed'}
    $cargoMetadata=(& cargo metadata --manifest-path $teamRoomManifest --format-version 1 --no-deps | ConvertFrom-Json)
    $teamRoomExe=Join-Path $cargoMetadata.target_directory 'release\ai-team-room-service.exe'
    Assert-Check (Test-Path $teamRoomExe) 'TEAM_ROOM_BINARY_PRESENT'
    $env:DATABASE_URL="postgres://postgres:$pgPassword@127.0.0.1:5432/team_room_p238b"
    $env:TEAM_ROOM_BIND='127.0.0.1:8082';$env:RUST_LOG='info'
    $teamRoomProcess=Start-Process -FilePath $teamRoomExe -WorkingDirectory $env:GITHUB_WORKSPACE -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $evidence 'team-room-full.stdout.log') -RedirectStandardError (Join-Path $evidence 'team-room-full.stderr.log')
    Remove-Item Env:DATABASE_URL,Env:TEAM_ROOM_BIND,Env:RUST_LOG -ErrorAction SilentlyContinue
    Wait-Http 'http://127.0.0.1:8082/health/ready' 90 'TEAM_ROOM_POSTGRES_READY'
    Write-Host ('TEAM_ROOM_FULL_PROCESS_ID='+$teamRoomProcess.Id)

    $p238aScript=Join-Path $env:GITHUB_WORKSPACE 'scripts\phone-02-p238-a-tls-e2e.ps1'
    Assert-Check (Test-Path $p238aScript) 'P238A_TLS_HARNESS_PRESENT'
    & $p238aScript
    if($LASTEXITCODE -ne 0){throw ('P2.38-A harness returned exit code '+$LASTEXITCODE)}
    Write-Host 'P238A_APPCONTAINER_SUPERVISOR_TLS_HARNESS=COMPLETED'
}
catch {
    $failure=$_
    Write-Host ('P238B_RUNNER_FAILURE='+$_.Exception.Message)
}
finally {
    Remove-Item Env:PGPASSWORD,Env:DATABASE_URL,Env:TEAM_ROOM_BIND,Env:RUST_LOG,Env:KC_BOOTSTRAP_ADMIN_USERNAME,Env:KC_BOOTSTRAP_ADMIN_PASSWORD,Env:KC_HEALTH_ENABLED,Env:KC_HTTP_ENABLED -ErrorAction SilentlyContinue
    Stop-TrackedProcess $teamRoomProcess 'TEAM_ROOM_FULL_PROCESS_STOP'
    if($keycloakProcess) {
        $kcDir=Join-Path $root 'keycloak-26.3.5'
        $kcChildren=@(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {$_.CommandLine -and $_.CommandLine.Contains($kcDir)})
        foreach($child in $kcChildren){Stop-Process -Id $child.ProcessId -Force -ErrorAction SilentlyContinue}
        Start-Sleep -Seconds 2
        $kcRemaining=@(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {$_.CommandLine -and $_.CommandLine.Contains($kcDir)})
        $cleanup.Add('KEYCLOAK_CHILD_PROCESSES_STOP='+$(if($kcRemaining.Count -eq 0){'PASS'}else{'FAIL'}))
        Stop-TrackedProcess $keycloakProcess 'KEYCLOAK_LAUNCHER_STOP'
    } else {$cleanup.Add('KEYCLOAK_LAUNCHER_STOP=NOT_STARTED')}
    if($pgService) {
        try {Stop-Service -Name $pgService.Name -Force -ErrorAction Stop;$cleanup.Add('POSTGRESQL_SERVICE_STOP=PASS')} catch {$cleanup.Add('POSTGRESQL_SERVICE_STOP=FAIL')}
    } else {$cleanup.Add('POSTGRESQL_SERVICE_STOP=NOT_STARTED')}
    if($pgInstalled) {
        try {& choco uninstall postgresql16 -y --no-progress --limit-output | Out-Null;if($LASTEXITCODE -ne 0){throw 'Chocolatey uninstall returned non-zero'};$cleanup.Add('POSTGRESQL_PACKAGE_UNINSTALL=PASS')} catch {$cleanup.Add('POSTGRESQL_PACKAGE_UNINSTALL=FAIL')}
    } else {$cleanup.Add('POSTGRESQL_PACKAGE_UNINSTALL=NOT_INSTALLED')}
    try {Stop-Transcript | Out-Null} catch {}
    foreach($file in Get-ChildItem $evidence -File -ErrorAction SilentlyContinue) {
        try {
            $raw=Get-Content $file.FullName -Raw -ErrorAction Stop
            $raw=$raw.Replace($pgPassword,'[REDACTED]').Replace($kcPassword,'[REDACTED]')
            $raw=[regex]::Replace($raw,'eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}','[REDACTED_JWT]')
            Set-Content -Path $file.FullName -Value $raw -Encoding utf8
        } catch {}
    }
    $p238aEvidence=Join-Path $env:RUNNER_TEMP 'phone02-p238a-evidence'
    if(Test-Path $p238aEvidence){Copy-Item (Join-Path $p238aEvidence '*') $evidence -Recurse -Force -ErrorAction SilentlyContinue}
    $pgLog=Join-Path $env:ProgramFiles 'PostgreSQL\16\data\log'
    if(Test-Path $pgLog){Get-ChildItem $pgLog -File -ErrorAction SilentlyContinue | Select-Object -Last 5 | Copy-Item -Destination $evidence -Force -ErrorAction SilentlyContinue}
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
    foreach($file in Get-ChildItem $evidence -File -ErrorAction SilentlyContinue) {
        try {
            $raw=Get-Content $file.FullName -Raw -ErrorAction Stop
            $raw=$raw.Replace($pgPassword,'[REDACTED]').Replace($kcPassword,'[REDACTED]')
            $raw=[regex]::Replace($raw,'eyJ[A-Za-z0-9_-]{10,}\\.[A-Za-z0-9_-]{10,}\\.[A-Za-z0-9_-]{10,}','[REDACTED_JWT]')
            Set-Content -Path $file.FullName -Value $raw -Encoding utf8
        } catch {}
    }
    try {Remove-Item $root -Recurse -Force -ErrorAction Stop;$cleanup.Add('EPHEMERAL_DOWNLOAD_DIRECTORY_REMOVED=PASS')} catch {$cleanup.Add('EPHEMERAL_DOWNLOAD_DIRECTORY_REMOVED=FAIL')}
    if($cleanup -match '=FAIL' -and $null -eq $failure){$failure=[System.Exception]::new('One or more ephemeral cleanup checks failed')}
    $hashLines=Get-ChildItem $evidence -File | Where-Object Name -ne 'SHA256SUMS' | ForEach-Object {(Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()+'  '+$_.Name}
    $hashLines | Set-Content (Join-Path $evidence 'SHA256SUMS') -Encoding ascii
}
if($null -ne $failure){throw $failure}
Write-Host 'P238B_RUNNER_BOOTSTRAP=COMPLETED'
