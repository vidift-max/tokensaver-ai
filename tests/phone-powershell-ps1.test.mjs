import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync, mkdtempSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { runPowerShellScript } from "../scripts/phone-windows-acl.mjs";

const probe = String.raw`$ErrorActionPreference = 'Stop'
Write-Output 'PHONE_PS1_BEGIN'
$source = @'
public static class PhoneAclPs1Probe
{
    public static string Ping() { return "OK"; }
}
'@
Add-Type -TypeDefinition $source -Language CSharp -ErrorAction Stop
if ([PhoneAclPs1Probe]::Ping() -ne 'OK') {
    throw 'C# Add-Type probe returned an unexpected value'
}
Write-Output 'PHONE_PS1_END'
exit 0
`;

test("Windows temporary PS1 executes Add-Type end to end and is removed", t => {
  if (process.platform !== "win32") return t.skip("Real PowerShell PS1 probe requires Windows CI");
  let scriptPath;
  const result = runPowerShellScript(probe, {
    timeout: 60000,
    onTemporaryScriptCreated(file) {
      scriptPath = file;
      assert.ok(existsSync(file));
      assert.match(file, /tokensaver-phone-acl/);
      assert.equal(readFileSync(file, "utf8"), probe);
    }
  });
  assert.equal(result.status, 0, "stdout:\n" + result.stdout + "\nstderr:\n" + result.stderr);
  assert.match(result.stdout, /PHONE_PS1_BEGIN/);
  assert.match(result.stdout, /PHONE_PS1_END/);
  assert.equal(String(result.stderr ?? ""), "");
  assert.ok(scriptPath);
  assert.equal(existsSync(scriptPath), false, "Temporary .ps1 must be removed after execution");
  console.log("PHONE_PS1_PROBE=PASS; exitCode=0; markers=BEGIN,END; cleanup=PASS");
});

test("temporary PS1 is removed after process failure and timeout", () => {
  for (const simulated of [
    { status: 1, signal: null, stdout: "begin", stderr: "script failed" },
    { status: null, signal: "SIGTERM", stdout: "begin", stderr: "timeout", error: Object.assign(new Error("timed out"), { code: "ETIMEDOUT" }) }
  ]) {
    let scriptPath;
    const root = mkdtempSync(path.join(os.tmpdir(), "phone-ps1-cleanup-test-"));
    try {
      if (simulated.error) {
        assert.throws(() => runPowerShellScript(probe, {
          tempRoot: root,
          onTemporaryScriptCreated(file) { scriptPath = file; },
          spawnSync() { return simulated; }
        }), error => error.cause === simulated.error && error.stdout === "begin" && error.stderr === "timeout");
      } else {
        const result = runPowerShellScript(probe, {
          tempRoot: root,
          onTemporaryScriptCreated(file) { scriptPath = file; },
          spawnSync() { return simulated; }
        });
        assert.equal(result.status, 1);
      }
      assert.ok(scriptPath);
      assert.equal(existsSync(scriptPath), false);
      assert.equal(existsSync(path.dirname(scriptPath)), false);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});
