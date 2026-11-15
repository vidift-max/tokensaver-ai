import test from "node:test";
import assert from "node:assert/strict";
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
      assert.match(script, /SetAccessRuleProtection\(\$true, \$false\)/);
      assert.match(script, /S-1-5-18/);
      assert.match(script, /S-1-5-32-544/);
      assert.match(script, /PHONE_RESULT_ACL_OK/);
      assert.equal(options.env.TOKENSAVER_PHONE_RESULT_DIRECTORY, "C:\\safe\\results");
      return "PHONE_RESULT_ACL_OK";
    }
  });
  assert.equal(captured.command, "powershell.exe");
  assert.equal(captured.args.includes("-EncodedCommand"), true);
  assert.equal(captured.options.timeout, 15000);
});

test("Windows ACL helper fails closed if ACL application or verification fails", () => {
  assert.throws(
    () => secureWindowsResultDirectory("C:\\unsafe\\results", {
      platform: "win32",
      execFileSync() { throw new Error("access denied"); }
    }),
    error => error.code === "PHONE_RESULT_WINDOWS_ACL_FAILED"
  );
});

test("Windows ACL helper requires a result directory", () => {
  assert.throws(
    () => secureWindowsResultDirectory("", { platform: "win32", execFileSync() {} }),
    /result directory is required/
  );
});

test("real Windows ACL application and verification", async t => {
  if (process.platform !== "win32") return t.skip("Windows ACL integration requires a Windows runner");
  const root = await mkdtemp(path.join(os.tmpdir(), "tokensaver-phone-acl-"));
  try {
    const directory = path.join(root, "private-results");
    await mkdir(directory);
    await writeFile(path.join(directory, "existing-record.json"), '{"encrypted":true}');
    secureWindowsResultDirectory(directory);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
