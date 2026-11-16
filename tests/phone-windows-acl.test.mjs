import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, mkdir, rm, symlink, writeFile } from "node:fs/promises";
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
      assert.match(script, /Import-Module Microsoft\.PowerShell\.Security -ErrorAction Stop/);
      assert.doesNotMatch(script, /Import-Module Microsoft\.PowerShell\.Security -Force/);
      assert.match(script, /SetAccessRuleProtection\(\$true, \$false\)/);
      assert.match(script, /Duplicate ACL entry for SID/);
      assert.match(script, /ACL SID set does not exactly match/);
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

test("real Windows ACL hardening rejects a directory junction (reparse point)", async t => {
  if (process.platform !== "win32") return t.skip("Windows reparse-point integration requires a Windows runner");
  const root = await mkdtemp(path.join(os.tmpdir(), "tokensaver-phone-reparse-"));
  try {
    const directory = path.join(root, "private-results");
    const outside = path.join(root, "outside");
    await mkdir(directory);
    await mkdir(outside);
    const junction = path.join(directory, "junction-outside");
    try {
      await symlink(outside, junction, "junction");
    } catch (error) {
      if (["EPERM", "EACCES", "ENOTSUP"].includes(error.code)) {
        return t.skip("Runner does not permit creating a directory junction");
      }
      throw error;
    }
    assert.throws(
      () => secureWindowsResultDirectory(directory),
      error => error.code === "PHONE_RESULT_WINDOWS_ACL_FAILED" && /Reparse point found/.test(error.message)
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
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
$root = [System.IO.Path]::GetFullPath($env:TOKENSAVER_PHONE_RESULT_DIRECTORY)
$currentSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$allowed = @($currentSid, 'S-1-5-18', 'S-1-5-32-544')
$existingPaths = @($env:TOKENSAVER_PHONE_EXISTING_ITEMS | ConvertFrom-Json)
$items = @((Get-Item -LiteralPath $root -Force)) + @(Get-ChildItem -LiteralPath $root -Force -Recurse)
foreach ($item in $items) {
  if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "Unexpected reparse point in ACL integration test: $($item.FullName)"
  }
  $acl = Get-Acl -LiteralPath $item.FullName
  $isExisting = $existingPaths -contains $item.FullName
  if ($isExisting -and -not $acl.AreAccessRulesProtected) {
    throw "Existing item must have protected ACL inheritance: $($item.FullName)"
  }
  if (-not $isExisting -and $acl.AreAccessRulesProtected) {
    throw "New item should inherit the protected parent ACL: $($item.FullName)"
  }
  $entries = @($acl.Access)
  if ($entries.Count -ne 3) {
    throw "Expected exactly three ACL entries on $($item.FullName), found $($entries.Count)"
  }
  $seenSids = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
  foreach ($entry in $entries) {
    $sid = $entry.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value
    if (-not $seenSids.Add($sid)) { throw "Duplicate SID $sid on $($item.FullName)" }
    if ($allowed -notcontains $sid -or
        $entry.AccessControlType -ne [System.Security.AccessControl.AccessControlType]::Allow -or
        $entry.FileSystemRights -ne [System.Security.AccessControl.FileSystemRights]::FullControl) {
      throw "Unexpected ACL entry on $($item.FullName) for $sid"
    }
    if ($isExisting -and $entry.IsInherited) { throw "Existing ACL entry unexpectedly inherited on $($item.FullName)" }
    if (-not $isExisting -and -not $entry.IsInherited) { throw "New item ACL entry is not inherited on $($item.FullName)" }
  }
  if ($seenSids.Count -ne 3 -or @($allowed | Where-Object { -not $seenSids.Contains($_) }).Count -ne 0) {
    throw "SID set mismatch on $($item.FullName)"
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
      env: {
        ...process.env,
        TOKENSAVER_PHONE_RESULT_DIRECTORY: directory,
        TOKENSAVER_PHONE_EXISTING_ITEMS: JSON.stringify([
          directory, nested,
          path.join(directory, "existing-record.json"),
          path.join(nested, "nested-record.json")
        ])
      },
      stdio: ["ignore", "pipe", "pipe"]
    });
    assert.match(output, /PHONE_RESULT_ACL_AUDIT_OK/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
