import { spawnSync } from "node:child_process";
import {
  chmodSync, closeSync, existsSync, fsyncSync, lstatSync, mkdtempSync,
  openSync, rmdirSync, unlinkSync, writeFileSync
} from "node:fs";
import os from "node:os";
import path from "node:path";

const POWERSHELL_SCRIPT = String.raw`
$ErrorActionPreference = 'Stop'
$target = [System.IO.Path]::GetFullPath($env:TOKENSAVER_PHONE_RESULT_DIRECTORY)
if ([string]::IsNullOrWhiteSpace($target) -or -not [System.IO.Directory]::Exists($target)) {
  throw 'Phone result directory does not exist or is invalid'
}
$volumeRoot = [System.IO.Path]::GetPathRoot($target)
if ($target.TrimEnd([char[]]@('\', '/')) -eq $volumeRoot.TrimEnd([char[]]@('\', '/'))) {
  throw 'Refusing to apply result ACL policy to a volume root'
}
$currentSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$allowed = @($currentSid, 'S-1-5-18', 'S-1-5-32-544')
if (@($allowed | Select-Object -Unique).Count -ne 3) {
  throw 'Current process SID overlaps a built-in allowed SID; refusing ambiguous ACL policy'
}

$nativeAclSource = @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;

public static class PhoneAclNative
{
    private const uint OWNER_SECURITY_INFORMATION = 0x00000001;
    private const uint DACL_SECURITY_INFORMATION = 0x00000004;
    private const uint PROTECTED_DACL_SECURITY_INFORMATION = 0x80000000;
    private const int ERROR_INSUFFICIENT_BUFFER = 122;
    private const int FULL_CONTROL = 0x001F01FF;

    [DllImport("advapi32.dll", EntryPoint = "GetFileSecurityW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool GetFileSecurity(string fileName, uint requestedInformation, IntPtr securityDescriptor, uint length, out uint needed);

    [DllImport("advapi32.dll", EntryPoint = "SetNamedSecurityInfoW", CharSet = CharSet.Unicode, SetLastError = false)]
    private static extern uint SetNamedSecurityInfo(string objectName, int objectType, uint securityInformation,
        IntPtr owner, IntPtr group, IntPtr dacl, IntPtr sacl);

    private static RawSecurityDescriptor ReadDescriptor(string path)
    {
        uint needed;
        bool first = GetFileSecurity(path, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION, IntPtr.Zero, 0, out needed);
        int error = Marshal.GetLastWin32Error();
        if (first || error != ERROR_INSUFFICIENT_BUFFER || needed == 0)
            throw new Win32Exception(error, "GetFileSecurityW size query failed for " + path);

        IntPtr buffer = Marshal.AllocHGlobal(checked((int)needed));
        try
        {
            if (!GetFileSecurity(path, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION, buffer, needed, out needed))
                throw new Win32Exception(Marshal.GetLastWin32Error(), "GetFileSecurityW failed for " + path);
            byte[] binary = new byte[needed];
            Marshal.Copy(buffer, binary, 0, checked((int)needed));
            return new RawSecurityDescriptor(binary, 0);
        }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }
    }

    private static DiscretionaryAcl BuildDacl(bool isDirectory, string[] allowedSids)
    {
        if (allowedSids == null || allowedSids.Length != 3)
            throw new InvalidOperationException("Exactly three authorized SIDs are required");

        DiscretionaryAcl dacl = new DiscretionaryAcl(isDirectory, false, allowedSids.Length);
        InheritanceFlags inheritance = isDirectory
            ? (InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit)
            : InheritanceFlags.None;
        foreach (string sidText in allowedSids)
        {
            SecurityIdentifier sid = new SecurityIdentifier(sidText);
            dacl.AddAccess(AccessControlType.Allow, sid, FULL_CONTROL, inheritance, PropagationFlags.None);
        }

        // AddAccess may merge equivalent entries; fail closed unless all three distinct SIDs
        // remain represented by exactly one ACE each.
        if (dacl.Count != 3)
            throw new InvalidOperationException("ACL construction merged or omitted an authorized SID");
        return dacl;
    }

    public static void VerifyConstructedDacl(bool isDirectory, string[] allowedSids)
    {
        DiscretionaryAcl dacl = BuildDacl(isDirectory, allowedSids);
        System.Collections.Generic.HashSet<string> expected =
            new System.Collections.Generic.HashSet<string>(allowedSids, StringComparer.OrdinalIgnoreCase);
        if (expected.Count != 3)
            throw new InvalidOperationException("Authorized SID list contains duplicates");

        AceFlags expectedFlags = isDirectory
            ? (AceFlags.ContainerInherit | AceFlags.ObjectInherit)
            : AceFlags.None;
        System.Collections.Generic.HashSet<string> observed =
            new System.Collections.Generic.HashSet<string>(StringComparer.OrdinalIgnoreCase);
        for (int i = 0; i < dacl.Count; i++)
        {
            GenericAce generic = dacl[i];
            QualifiedAce ace = generic as QualifiedAce;
            if (ace == null || generic.AceType != AceType.AccessAllowed ||
                ace.AceQualifier != AceQualifier.AccessAllowed || ace.IsCallback ||
                ace.AccessMask != FULL_CONTROL || generic.AceFlags != expectedFlags)
                throw new InvalidOperationException("Constructed DACL contains an unexpected ACE");
            string sid = ace.SecurityIdentifier.Value;
            if (!expected.Contains(sid) || !observed.Add(sid))
                throw new InvalidOperationException("Constructed DACL contains an extra or duplicate SID");
        }
        if (observed.Count != 3 || !expected.SetEquals(observed))
            throw new InvalidOperationException("Constructed DACL does not contain exactly the authorized SIDs");
    }

    public static void Apply(string path, bool isDirectory, string[] allowedSids)
    {
        DiscretionaryAcl dacl = BuildDacl(isDirectory, allowedSids);
        byte[] binary = new byte[dacl.BinaryLength];
        dacl.GetBinaryForm(binary, 0);

        IntPtr buffer = Marshal.AllocHGlobal(binary.Length);
        try
        {
            Marshal.Copy(binary, 0, buffer, binary.Length);
            uint securityInformation = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
            const int SE_FILE_OBJECT = 1;
            uint result = SetNamedSecurityInfo(path, SE_FILE_OBJECT, securityInformation,
                IntPtr.Zero, IntPtr.Zero, buffer, IntPtr.Zero);
            if (result != 0)
                throw new Win32Exception(checked((int)result), "SetNamedSecurityInfoW failed for " + path);
        }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }

        Verify(path, isDirectory, allowedSids, true, false);
    }

    public static void Verify(string path, bool isDirectory, string[] allowedSids, bool expectProtected, bool expectInherited)
    {
        RawSecurityDescriptor descriptor = ReadDescriptor(path);
        bool protectedDacl = (descriptor.ControlFlags & ControlFlags.DiscretionaryAclProtected) != 0;
        if (protectedDacl != expectProtected)
            throw new InvalidOperationException("Unexpected DACL protection state for " + path);
        if (descriptor.DiscretionaryAcl == null)
            throw new InvalidOperationException("DACL is absent for " + path);

        if (descriptor.DiscretionaryAcl.Count != 3)
            throw new InvalidOperationException("Expected exactly three ACEs for " + path);

        System.Collections.Generic.HashSet<string> expected =
            new System.Collections.Generic.HashSet<string>(allowedSids, StringComparer.OrdinalIgnoreCase);
        if (descriptor.Owner == null || !expected.Contains(descriptor.Owner.Value))
            throw new InvalidOperationException("Object owner is outside the three authorized SIDs for " + path);
        System.Collections.Generic.HashSet<string> observed =
            new System.Collections.Generic.HashSet<string>(StringComparer.OrdinalIgnoreCase);

        AceFlags inheritanceFlags = isDirectory
            ? (AceFlags.ContainerInherit | AceFlags.ObjectInherit)
            : AceFlags.None;

        for (int i = 0; i < descriptor.DiscretionaryAcl.Count; i++)
        {
            GenericAce generic = descriptor.DiscretionaryAcl[i];
            QualifiedAce ace = generic as QualifiedAce;
            if (ace == null || generic.AceType != AceType.AccessAllowed || ace.AceQualifier != AceQualifier.AccessAllowed ||
                ace.IsCallback || ace.AccessMask != FULL_CONTROL)
                throw new InvalidOperationException("Unexpected ACL ACE type or rights for " + path);

            AceFlags flags = generic.AceFlags;
            if (!expectInherited)
            {
                if (flags != inheritanceFlags)
                    throw new InvalidOperationException("Unexpected explicit ACL inheritance flags for " + path);
            }
            else
            {
                if ((flags & AceFlags.Inherited) == 0 ||
                    (flags & (AceFlags.InheritOnly | AceFlags.NoPropagateInherit)) != 0)
                    throw new InvalidOperationException("Expected an effective inherited ACE for " + path);
                if (isDirectory && (flags & inheritanceFlags) != inheritanceFlags)
                    throw new InvalidOperationException("Inherited directory ACE must continue to protect descendants: " + path);
                if (!isDirectory && (flags & AceFlags.ContainerInherit) != 0)
                    throw new InvalidOperationException("File ACE cannot carry container inheritance: " + path);
                if (!isDirectory && (flags & ~(AceFlags.Inherited | AceFlags.ObjectInherit)) != 0)
                    throw new InvalidOperationException("Unexpected file inheritance flags for " + path);
            }

            string sid = ace.SecurityIdentifier.Value;
            if (!expected.Contains(sid) || !observed.Add(sid))
                throw new InvalidOperationException("Unexpected or duplicate ACL SID " + sid + " for " + path);
        }

        if (observed.Count != 3 || !expected.SetEquals(observed))
            throw new InvalidOperationException("ACL SID set does not exactly match the authorized set for " + path);
    }
}
'@
Write-Output ("PHONE_RESULT_ACL_RUNTIME=" + $PSVersionTable.PSVersion.ToString() + "; Edition=" + $PSVersionTable.PSEdition + "; CLR=" + [System.Environment]::Version.ToString())
Add-Type -TypeDefinition $nativeAclSource -Language CSharp -ErrorAction Stop

if ($env:TOKENSAVER_PHONE_ACL_CONSTRUCTION_TEST) {
  [PhoneAclNative]::VerifyConstructedDacl($false, $allowed)
  [PhoneAclNative]::VerifyConstructedDacl($true, $allowed)
  Write-Output 'PHONE_RESULT_ACL_CONSTRUCTION_OK'
  exit 0
}

if ($env:TOKENSAVER_PHONE_ACL_AUDIT_ITEMS) {
  $existingPaths = ConvertFrom-Json -InputObject $env:TOKENSAVER_PHONE_ACL_EXISTING_ITEMS
  $auditPaths = ConvertFrom-Json -InputObject $env:TOKENSAVER_PHONE_ACL_AUDIT_ITEMS
  foreach ($path in $auditPaths) {
    try {
      $attributes = [System.IO.File]::GetAttributes($path)
    } catch {
      $pathText = [string]$path
      $pathUtf16 = [Convert]::ToBase64String([System.Text.Encoding]::Unicode.GetBytes($pathText))
      Write-Output ("PHONE_ACL_AUDIT_PATH_TYPE=" + $path.GetType().FullName)
      Write-Output ("PHONE_ACL_AUDIT_PATH_LENGTH=" + $pathText.Length)
      Write-Output ("PHONE_ACL_AUDIT_PATH_UTF16_BASE64=" + $pathUtf16)
      throw
    }
    if (($attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
      throw "Reparse point found during ACL audit: $path"
    }
    $isDirectory = ($attributes -band [System.IO.FileAttributes]::Directory) -ne 0
    $wasExisting = $existingPaths -contains $path
    [PhoneAclNative]::Verify($path, $isDirectory, $allowed, $wasExisting, (-not $wasExisting))
  }
  Write-Output 'PHONE_RESULT_ACL_AUDIT_OK'
  exit 0
}

$pending = [System.Collections.Generic.Stack[string]]::new()
$pending.Push($target)
while ($pending.Count -gt 0) {
  $current = $pending.Pop()
  $attributes = [System.IO.File]::GetAttributes($current)
  if (($attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "Reparse point found in result storage; refusing to follow it: $current"
  }
  $isDirectory = ($attributes -band [System.IO.FileAttributes]::Directory) -ne 0
  [PhoneAclNative]::Apply($current, $isDirectory, $allowed)
  if ($isDirectory) {
    foreach ($child in [System.IO.Directory]::EnumerateFileSystemEntries($current)) {
      $childAttributes = [System.IO.File]::GetAttributes($child)
      if (($childAttributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Reparse point found in result storage; refusing to follow it: $child"
      }
      if (($childAttributes -band [System.IO.FileAttributes]::Directory) -ne 0) {
        $pending.Push($child)
      } else {
        [PhoneAclNative]::Apply($child, $false, $allowed)
      }
    }
  }
}
Write-Output 'PHONE_RESULT_ACL_OK'
`;

export function runPowerShellScript(script, options = {}) {
  if (typeof script !== "string" || script.length === 0) {
    throw new TypeError("A non-empty PowerShell script is required");
  }
  const tempRoot = options.tempRoot ?? os.tmpdir();
  let tempDirectory;
  let scriptPath;
  let fd;
  let result;
  let primaryError;

  try {
    // mkdtemp creates a new directory atomically with an unpredictable suffix.
    tempDirectory = mkdtempSync(path.join(tempRoot, "tokensaver-phone-acl-"));
    const directoryStat = lstatSync(tempDirectory);
    if (!directoryStat.isDirectory() || directoryStat.isSymbolicLink()) {
      throw new Error("Temporary PowerShell directory is not a plain directory");
    }

    scriptPath = path.join(tempDirectory, "acl-helper.ps1");
    // wx prevents replacement/reuse; the parent directory is exclusively created above.
    fd = openSync(scriptPath, "wx", 0o600);
    const fileStat = lstatSync(scriptPath);
    if (!fileStat.isFile() || fileStat.isSymbolicLink()) {
      throw new Error("Temporary PowerShell script is not a plain file");
    }
    try {
      writeFileSync(fd, script, { encoding: "utf8" });
      fsyncSync(fd);
    } finally {
      closeSync(fd);
      fd = undefined;
    }
    // On POSIX this tightens mode explicitly. On Windows, inherited ACLs from the
    // per-user temporary directory remain authoritative; chmod is best effort.
    try { chmodSync(scriptPath, 0o600); } catch (error) {
      if (process.platform !== "win32") throw error;
    }
    if (options.onTemporaryScriptCreated) options.onTemporaryScriptCreated(scriptPath);

    const runner = options.spawnSync ?? spawnSync;
    result = runner("powershell.exe", [
      "-NoLogo", "-NoProfile", "-NonInteractive", "-File", scriptPath
    ], {
      encoding: "utf8",
      windowsHide: true,
      timeout: 60000,
      env: { ...process.env, ...(options.env ?? {}) },
      stdio: ["ignore", "pipe", "pipe"]
    });

    if (result.error) {
      const error = new Error(
        "PowerShell process execution failed: " +
        "exitCode=" + String(result.status) +
        "\nstdout:\n" + String(result.stdout ?? "") +
        "\nstderr:\n" + String(result.stderr ?? "") +
        "\noriginal cause:\n" + (result.error?.stack || result.error?.message || String(result.error)),
        { cause: result.error }
      );
      error.code = "PHONE_RESULT_WINDOWS_ACL_FAILED";
      error.stdout = String(result.stdout ?? "");
      error.stderr = String(result.stderr ?? "");
      error.exitCode = result.status;
      throw error;
    }
    return result;
  } catch (error) {
    primaryError = error;
    throw error;
  } finally {
    let cleanupError;
    try {
      if (fd !== undefined) closeSync(fd);
      if (scriptPath && existsSync(scriptPath)) unlinkSync(scriptPath);
      if (tempDirectory && existsSync(tempDirectory)) rmdirSync(tempDirectory);
    } catch (error) {
      cleanupError = error;
    }
    if (cleanupError) {
      const message = "Temporary PowerShell cleanup failed: " +
        (cleanupError?.stack || cleanupError?.message || String(cleanupError));
      if (primaryError) {
        primaryError.cleanupError = cleanupError;
        primaryError.message += "\n" + message;
      } else {
        const error = new Error(message, { cause: cleanupError });
        error.code = "PHONE_RESULT_WINDOWS_ACL_CLEANUP_FAILED";
        throw error;
      }
    }
  }
}

function invokeAclScript(directory, extraEnvironment = {}, options = {}) {
  let result;
  try {
    result = runPowerShellScript(POWERSHELL_SCRIPT, {
      ...options,
      env: {
        TOKENSAVER_PHONE_RESULT_DIRECTORY: directory,
        ...extraEnvironment
      }
    });
  } catch (cause) {
    if (cause.code === "PHONE_RESULT_WINDOWS_ACL_FAILED") throw cause;
    const error = new Error(
      "Unable to create or execute temporary Windows ACL PowerShell script: " +
      (cause?.stack || cause?.message || String(cause)),
      { cause }
    );
    error.code = "PHONE_RESULT_WINDOWS_ACL_FAILED";
    throw error;
  }

  const stdout = String(result.stdout ?? "");
  const stderr = String(result.stderr ?? "");
  if (stdout) process.stdout.write(stdout.endsWith("\n") ? stdout : stdout + "\n");
  if (stderr) process.stderr.write(stderr.endsWith("\n") ? stderr : stderr + "\n");

  const expectedMarker = extraEnvironment.TOKENSAVER_PHONE_ACL_CONSTRUCTION_TEST
    ? "PHONE_RESULT_ACL_CONSTRUCTION_OK"
    : extraEnvironment.TOKENSAVER_PHONE_ACL_AUDIT_ITEMS
      ? "PHONE_RESULT_ACL_AUDIT_OK"
      : "PHONE_RESULT_ACL_OK";
  if (result.status !== 0 || stderr.length > 0 || !stdout.includes("PHONE_RESULT_ACL_RUNTIME=") || !stdout.includes(expectedMarker)) {
    const cause = stderr.length > 0
      ? new Error("PowerShell emitted stderr despite exit code " + String(result.status))
      : new Error("PowerShell did not emit required marker " + expectedMarker);
    const detail = [
      "expectedMarker=" + expectedMarker,
      "exitCode=" + String(result.status),
      "signal=" + String(result.signal ?? ""),
      "stdout:",
      stdout,
      "stderr:",
      stderr,
      "cause:",
      cause.stack || cause.message
    ].join("\n");
    const error = new Error(
      "Unable to apply and verify restrictive Windows ACLs for Phone-02 result storage: " + detail,
      { cause }
    );
    error.code = "PHONE_RESULT_WINDOWS_ACL_FAILED";
    error.exitCode = result.status;
    error.stdout = stdout;
    error.stderr = stderr;
    throw error;
  }

  return stdout;
}

export function secureWindowsResultDirectory(directory, options = {}) {
  if (process.platform !== "win32" && options.platform !== "win32") return;
  if (typeof directory !== "string" || !directory.trim()) {
    throw new TypeError("A result directory is required for Windows ACL protection");
  }
  const output = invokeAclScript(directory, {}, options);
  if (!output.includes("PHONE_RESULT_ACL_OK")) {
    const error = new Error("Windows ACL verification marker missing");
    error.code = "PHONE_RESULT_WINDOWS_ACL_FAILED";
    throw error;
  }
}

export function verifyWindowsAclConstruction(directory, options = {}) {
  if (process.platform !== "win32" && options.platform !== "win32") return;
  if (typeof directory !== "string" || !directory.trim()) {
    throw new TypeError("A result directory is required for Windows ACL construction verification");
  }
  const output = invokeAclScript(directory, {
    TOKENSAVER_PHONE_ACL_CONSTRUCTION_TEST: "1"
  }, options);
  if (!output.includes("PHONE_RESULT_ACL_CONSTRUCTION_OK")) {
    const error = new Error("Windows ACL construction verification marker missing");
    error.code = "PHONE_RESULT_WINDOWS_ACL_FAILED";
    throw error;
  }
}

export function verifyWindowsResultAcl(directory, existingPaths, auditPaths, options = {}) {
  if (process.platform !== "win32" && options.platform !== "win32") return;
  if (typeof directory !== "string" || !directory.trim()) {
    throw new TypeError("A result directory is required for Windows ACL verification");
  }
  const output = invokeAclScript(directory, {
    TOKENSAVER_PHONE_ACL_EXISTING_ITEMS: JSON.stringify(existingPaths),
    TOKENSAVER_PHONE_ACL_AUDIT_ITEMS: JSON.stringify(auditPaths)
  }, options);
  if (!output.includes("PHONE_RESULT_ACL_AUDIT_OK")) {
    const error = new Error("Windows ACL audit marker missing");
    error.code = "PHONE_RESULT_WINDOWS_ACL_FAILED";
    throw error;
  }
}
