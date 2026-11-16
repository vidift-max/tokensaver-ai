import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";

test("Windows PowerShell stdin transport executes a C# Add-Type probe end to end", t => {
  if (process.platform !== "win32") return t.skip("PowerShell stdin probe requires the Windows CI runner");

  const script = String.raw`$ErrorActionPreference = 'Stop'
Write-Output 'PHONE_STDIN_BEGIN'
$source = @'
public static class PhoneAclStdinProbe
{
    public static string Ping() { return "OK"; }
}
'@
Add-Type -TypeDefinition $source -Language CSharp -ErrorAction Stop
if ([PhoneAclStdinProbe]::Ping() -ne 'OK') {
    throw 'C# Add-Type probe returned an unexpected value'
}
Write-Output 'PHONE_STDIN_END'
exit 0
`;

  const result = spawnSync("powershell.exe", [
    "-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "-"
  ], {
    encoding: "utf8",
    input: script,
    windowsHide: true,
    timeout: 15000,
    stdio: ["pipe", "pipe", "pipe"]
  });

  const stdout = String(result.stdout ?? "");
  const stderr = String(result.stderr ?? "");
  assert.equal(result.error, undefined, "spawnSync error: " + String(result.error));
  assert.equal(
    result.status,
    0,
    "PowerShell exitCode=" + String(result.status) +
    " signal=" + String(result.signal) +
    "\nstdout:\n" + stdout + "\nstderr:\n" + stderr
  );
  assert.match(stdout, /PHONE_STDIN_BEGIN/);
  assert.match(stdout, /PHONE_STDIN_END/);
  assert.equal(stderr, "", "Unexpected PowerShell stderr:\n" + stderr);
  console.log("PHONE_STDIN_PROBE=PASS; exitCode=0; stdoutMarkers=BEGIN,END; stderrBytes=0");
});
