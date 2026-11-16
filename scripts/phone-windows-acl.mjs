import { spawnSync } from "node:child_process";

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
  $existingPaths = @($env:TOKENSAVER_PHONE_ACL_EXISTING_ITEMS | ConvertFrom-Json)
  $auditPaths = @($env:TOKENSAVER_PHONE_ACL_AUDIT_ITEMS | ConvertFrom-Json)
  foreach ($path in $auditPaths) {
    $attributes = [System.IO.File]::GetAttributes($path)
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

function invokeAclScript(directory, extraEnvironment = {}, options = {}) {
  const runner = options.spawnSync ?? spawnSync;
  const input = POWERSHELL_SCRIPT;
  const result = runner("powershell.exe", [
    "-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "-"
  ], {
    encoding: "utf8",
    input,
    windowsHide: true,
    timeout: 60000,
    env: {
      ...process.env,
      TOKENSAVER_PHONE_RESULT_DIRECTORY: directory,
      ...extraEnvironment
    },
    stdio: ["pipe", "pipe", "pipe"]
  });

  const stdout = String(result.stdout ?? "");
  const stderr = String(result.stderr ?? "");
  const runtime = stdout.split(/\r?\n/).find(line => line.startsWith("PHONE_RESULT_ACL_RUNTIME="));
  if (runtime) process.stdout.write(runtime + "\n");
  if (stderr) process.stderr.write(stderr.endsWith("\n") ? stderr : stderr + "\n");

  if (result.error || result.status !== 0) {
    const cause = result.error ?? new Error("PowerShell exited with status " + String(result.status));
    const detail = [
      "exitCode=" + String(result.status),
      "signal=" + String(result.signal ?? ""),
      "stdout:",
      stdout,
      "stderr:",
      stderr,
      "cause:",
      cause?.stack || cause?.message || String(cause)
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
