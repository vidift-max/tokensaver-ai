import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { secureWindowsResultDirectory, verifyWindowsResultAcl } from "../scripts/phone-windows-acl.mjs";

test("Windows ACL helper builds a non-interpolated PowerShell command and verifies its marker", () => {
  let captured;
  secureWindowsResultDirectory("C:\\safe\\results", {
    platform: "win32",
    execFileSync(command, args, options) {
      captured = { command, args, options };
      const encoded = args[args.indexOf("-EncodedCommand") + 1];
      const script = Buffer.from(encoded, "base64").toString("utf16le");
      assert.doesNotMatch(script, /Import-Module Microsoft\.PowerShell\.Security/);
      assert.doesNotMatch(script, /\bGet-Acl\b|\bSet-Acl\b/);
      assert.match(script, /GetFileSecurityW/);
      assert.match(script, /SetFileSecurityW/);
      assert.match(script, /RawSecurityDescriptor/);
      assert.match(script, /DiscretionaryAcl/);
      assert.match(script, /Unexpected or duplicate ACL SID/);
      assert.match(script, /ACL SID set does not exactly match/);
      assert.match(script, /S-1-5-18/);
      assert.match(script, /S-1-5-32-544/);
      assert.match(script, /EnumerateFileSystemEntries/);
      assert.match(script, /PROTECTED_DACL_SECURITY_INFORMATION/);
      assert.match(script, /ReparsePoint/);
      assert.match(script, /PHONE_RESULT_ACL_OK/);
      assert.equal(options.env.TOKENSAVER_PHONE_RESULT_DIRECTORY, "C:\\safe\\results");
      return "PHONE_RESULT_ACL_OK";
    }
  });
  assert.equal(captured.command, "pwsh.exe");
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

test("Windows ACL helper rejects a missing or unexpected target path", async t => {
  if (process.platform !== "win32") return t.skip("Windows path validation requires a Windows runner");
  const root = await mkdtemp(path.join(os.tmpdir(), "tokensaver-phone-acl-path-"));
  try {
    await assert.rejects(
      async () => secureWindowsResultDirectory(path.join(root, "missing-target")),
      error => error.code === "PHONE_RESULT_WINDOWS_ACL_FAILED"
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

    const existingPaths = [
      directory, nested,
      path.join(directory, "existing-record.json"),
      path.join(nested, "nested-record.json")
    ];
    const auditPaths = [
      ...existingPaths,
      laterDirectory,
      path.join(directory, "created-after-hardening.json"),
      path.join(laterDirectory, "nested-new-file.json")
    ];
    verifyWindowsResultAcl(directory, existingPaths, auditPaths);  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
