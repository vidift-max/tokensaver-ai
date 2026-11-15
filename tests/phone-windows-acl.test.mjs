import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { secureWindowsResultDirectory } from "../scripts/phone-windows-acl.mjs";

test("Windows ACL helper builds a non-interpolated PowerShell command and verifies its marker", () => {
  let captured;
  secureWindowsResultDirectory("C:\\safe\\results", {
    platform: "win32",
    execFileSync(command, args, options) {
      captured = { command, args, options };
      const encoded = args[args.indexOf("-EncodedCommand") + 1];
      const script = Buffer.from(encoded, "base64").toString("utf16le");
      assert.match(script, /Import-Module Microsoft\\.PowerShell\\.Security -Force/);
      assert.match(script, /SetAccessRuleProtection\(\$true, \$false\)/);
      assert.match(script, /S-1-5-18/);
      assert.match(script, /S-1-5-32-544/);
      assert.match(script, /Get-ChildItem -LiteralPath \$current -Force/);
      assert.match(script, /ReparsePoint/);
      assert.match(script, /PHONE_RESULT_ACL_OK/);
      assert.equal(options.env.TOKENSAVER_PHONE_RESULT_DIRECTORY, "C:\\safe\\results");
      return "PHONE_RESULT_ACL_OK";
    }
  });
  assert.equal(captured.command, "powershell.exe");
  assert.equal(captured.args.includes("-EncodedCommand"), true);
  assert.equal(captured.options.timeout, 60000);
});

test("Windows ACL helper fails closed if ACL application or verification fails", () => {
  assert.throws(
    () => secureWindowsResultDirectory("C:\\unsafe\\results", {
      platform: "win32",
      execFileSync() { throw new Error("access denied"); }
    }),
    error => error.code === "PHONE_RESULT_WINDOWS_ACL_FAILED" && /access denied/.test(error.message)
  );
});

test("Windows ACL helper requires a result directory", () => {
  assert.throws(
    () => secureWindowsResultDirectory("", { platform: "win32", execFileSync() {} }),
    /result directory is required/
  );
});

test("real Windows ACL application protects existing nested items and newly inherited items", async t => {
  if (process.platform !== "win32") return t.skip("Windows ACL integration requires a Windows runner");
  const root = await mkdtemp(path.join(os.tmpdir(), "tokensaver-phone-acl-"));
  try {
    const directory = path.join(root, "private-results");
    const nested = path.join(directory, "nested");
    await mkdir(nested, { recursive: true });
    await writeFile(path.join(directory, "existing-record.json"), '{"encrypted":true}');
    await writeFile(path.join(nested, "nested-record.json"), '{"encrypted":true}');
    secureWindowsResultDirectory(directory);

    const laterDirectory = path.join(directory, "created-after-hardening");
    await mkdir(laterDirectory);
    await writeFile(path.join(directory, "created-after-hardening.json"), "new");
    await writeFile(path.join(laterDirectory, "nested-new-file.json"), "new");

    const audit = String.raw`
$ErrorActionPreference = 'Stop'
Import-Module Microsoft.PowerShell.Security -Force -ErrorAction Stop
$root = [System.IO.Path]::GetFullPath($env:TOKENSAVER_PHONE_RESULT_DIRECTORY)
$currentSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$allowed = @($currentSid, 'S-1-5-18', 'S-1-5-32-544')
$items = @((Get-Item -LiteralPath $root -Force)) + @(Get-ChildItem -LiteralPath $root -Force -Recurse)
foreach ($item in $items) {
  if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "Unexpected reparse point in ACL integration test: $($item.FullName)"
  }
  $acl = Get-Acl -LiteralPath $item.FullName
  $entries = @($acl.Access)
  if ($entries.Count -ne 3) {
    throw "Expected three effective ACL entries on $($item.FullName), found $($entries.Count)"
  }
  foreach ($entry in $entries) {
    $sid = $entry.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value
    if ($allowed -notcontains $sid -or
        $entry.AccessControlType -ne [System.Security.AccessControl.AccessControlType]::Allow -or
        $entry.FileSystemRights -ne [System.Security.AccessControl.FileSystemRights]::FullControl) {
      throw "Unexpected effective ACL entry on $($item.FullName) for $sid"
    }
  }
}
Write-Output 'PHONE_RESULT_ACL_AUDIT_OK'
`;
    const encoded = Buffer.from(audit, "utf16le").toString("base64");
    const output = execFileSync("powershell.exe", [
      "-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded
    ], {
      encoding: "utf8",
      windowsHide: true,
      timeout: 60000,
      env: { ...process.env, TOKENSAVER_PHONE_RESULT_DIRECTORY: directory },
      stdio: ["ignore", "pipe", "pipe"]
    });
    assert.match(output, /PHONE_RESULT_ACL_AUDIT_OK/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
