@echo off
setlocal
cd /d "%~dp0"
where node >nul 2>nul
if errorlevel 1 (
  echo ERROR: Node.js is required to run the Mission Runner.
  exit /b 9009
)
if not exist "scripts\mission-runner.mjs" (
  echo ERROR: Mission Runner entrypoint not found: scripts\mission-runner.mjs
  exit /b 2
)
node "scripts\mission-runner.mjs" %*
set "RUNNER_EXIT=%ERRORLEVEL%"
exit /b %RUNNER_EXIT%
