import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { secureWindowsResultDirectory, verifyWindowsAclConstruction, verifyWindowsResultAcl } from "../scripts/phone-windows-acl.mjs";

test("Windows ACL helper executes a temporary PS1 without exposing script contents in arguments", () => {
  let captured;
  secureWindowsResultDirectory("C:\\safe\\results", {
    platform: "win32",
    spawnSync(command, args, options) {
      captured = { command, args, options };
      const scriptPath = args[args.length - 1];
      assert.match(scriptPath, /tokensaver-phone-acl/);
      const script = options.env.TOKENSAVER_PHONE_RESULT_DIRECTORY;
      assert.doesNotMatch(script, /Import-Module Microsoft\.PowerShell\.Security/);
      assert.doesNotMatch(script, /\bGet-Acl\b|\bSet-Acl\b/);
      
      
      assert.doesNotMatch(script, /SetFileSecurityW/);
      
      assert.match(script, /new DiscretionaryAcl\(isDirectory, false, allowedSids\.Length\)/);
      assert.match(script, /dacl\.AddAccess\(AccessControlType\.Allow, sid, FULL_CONTROL, inheritance, PropagationFlags\.None\)/);
      assert.match(script, /dacl\.Count != 3/);
      
      
      assert.doesNotMatch(script, /\bInsertAce\b/);
      assert.match(script, /InheritanceFlags\.ContainerInherit \| InheritanceFlags\.ObjectInherit/);
      
      
      
      
      
      
      
      
      
      
      
      assert.equal(options.env.TOKENSAVER_PHONE_RESULT_DIRECTORY, "C:\\safe\\results");
      return { status: 0, signal: null, stdout: "PHONE_RESULT_ACL_RUNTIME=5.1; PHONE_RESULT_ACL_OK", stderr: "" };
    }
  });
  assert.equal(captured.command, "powershell.exe");
  assert.deepEqual(captured.args, ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "-"]);
  assert.equal(captured.args.join(" ").includes("PHONE_RESULT_ACL_RUNTIME"), false);
  
  assert.equal(captured.args.join(" ").length < 200, true);
  assert.equal(captured.options.timeout, 60000);
});

test("real Windows DACL construction independently verifies three exact ACEs in memory", async t => {
  if (process.platform !== "win32") return t.skip("Windows ACL construction requires a Windows runner");
  const root = await mkdtemp(path.join(os.tmpdir(), "tokensaver-phone-acl-build-"));
  try {
    verifyWindowsAclConstruction(root);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("Windows ACL helper fails closed if ACL application or verification fails", () => {
  assert.throws(
    () => secureWindowsResultDirectory("C:\\unsafe\\results", {
      platform: "win32",
      spawnSync() { return { status: 1, signal: null, stdout: "partial output", stderr: "access denied" }; }
    }),
    error => error.code === "PHONE_RESULT_WINDOWS_ACL_FAILED" && /access denied/.test(error.message)
  );
});

test("Windows ACL helper preserves complete PowerShell stdout and stderr on failure", () => {
  assert.throws(
    () => secureWindowsResultDirectory("C:\\\\unsafe\\\\results", {
      platform: "win32",
      spawnSync() {
        return {
          status: 1,
          signal: null,
          stdout: "STDOUT_FULL_DIAGNOSTIC",
          stderr: "STDERR_FULL_DIAGNOSTIC"
        };
      }
    }),
    error => error.code === "PHONE_RESULT_WINDOWS_ACL_FAILED" &&
      error.message.includes("STDOUT_FULL_DIAGNOSTIC") &&
      error.message.includes("STDERR_FULL_DIAGNOSTIC")
  );
});

test("Windows ACL helper fails closed when the success marker is absent", () => {
  assert.throws(
    () => secureWindowsResultDirectory("C:\\\\safe\\\\results", {
      platform: "win32",
      spawnSync() { return { status: 0, signal: null, stdout: "PHONE_RESULT_ACL_RUNTIME=5.1", stderr: "" }; }
    }),
    error => error.code === "PHONE_RESULT_WINDOWS_ACL_FAILED" && /PHONE_RESULT_ACL_OK/.test(error.message) && /PHONE_RESULT_ACL_RUNTIME=5.1/.test(error.message)
  );
});


test("Windows ACL helper treats PowerShell stderr as failure even when exit code is zero", () => {
  assert.throws(
    () => secureWindowsResultDirectory("C:\\safe\\results", {
      platform: "win32",
      spawnSync() {
        return { status: 0, signal: null, stdout: "PHONE_RESULT_ACL_RUNTIME=5.1", stderr: "Add-Type compilation error" };
      }
    }),
    error => error.code === "PHONE_RESULT_WINDOWS_ACL_FAILED" &&
      /exitCode=0/.test(error.message) &&
      /Add-Type compilation error/.test(error.message)
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

    const symlinkRoot = path.join(root, "file-symlink-case");
    const symlinkDirectory = path.join(symlinkRoot, "private-results");
    const fileTarget = path.join(symlinkRoot, "outside.txt");
    await mkdir(symlinkDirectory, { recursive: true });
    await writeFile(fileTarget, "outside");
    try {
      await symlink(fileTarget, path.join(symlinkDirectory, "outside-link.txt"), "file");
    } catch (error) {
      if (!["EPERM", "EACCES", "ENOTSUP"].includes(error.code)) throw error;
      return t.skip("Runner does not permit creating a file symlink");
    }
    assert.throws(
      () => secureWindowsResultDirectory(symlinkDirectory),
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
    () => secureWindowsResultDirectory("", { platform: "win32", spawnSync() {} }),
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
    verifyWindowsResultAcl(directory, existingPaths, auditPaths);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
