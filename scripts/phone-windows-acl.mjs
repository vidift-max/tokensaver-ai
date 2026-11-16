import { execFileSync } from "node:child_process";

const POWERSHELL_SCRIPT = String.raw`
$ErrorActionPreference = 'Stop'
# Load the security cmdlets explicitly, without -Force (which reloads an already loaded module).
Import-Module Microsoft.PowerShell.Security -ErrorAction Stop
$target = [System.IO.Path]::GetFullPath($env:TOKENSAVER_PHONE_RESULT_DIRECTORY)
if (-not (Test-Path -LiteralPath $target -PathType Container)) {
  throw 'Phone result directory does not exist'
}
$currentSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$allowed = @($currentSid, 'S-1-5-18', 'S-1-5-32-544')
$uniqueAllowed = @($allowed | Select-Object -Unique)
if ($uniqueAllowed.Count -ne 3) {
  throw 'Current process SID overlaps a built-in allowed SID; refusing ambiguous ACL policy'
}
$inheritance = [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [System.Security.AccessControl.InheritanceFlags]::ObjectInherit
$fullControl = [System.Security.AccessControl.FileSystemRights]::FullControl
$pending = [System.Collections.Generic.Stack[string]]::new()
$pending.Push($target)

function Set-RestrictedAcl([string]$path, [bool]$isDirectory) {
  $acl = Get-Acl -LiteralPath $path -ErrorAction Stop
  $acl.SetAccessRuleProtection($true, $false)
  foreach ($existing in @($acl.Access)) {
    [void]$acl.RemoveAccessRuleAll($existing)
  }
  foreach ($sid in $allowed) {
    $identity = [System.Security.Principal.SecurityIdentifier]::new($sid)
    if ($isDirectory) {
      $rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
        $identity, $fullControl, $inheritance,
        [System.Security.AccessControl.PropagationFlags]::None,
        [System.Security.AccessControl.AccessControlType]::Allow
      )
    } else {
      $rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
        $identity, $fullControl,
        [System.Security.AccessControl.AccessControlType]::Allow
      )
    }
    [void]$acl.AddAccessRule($rule)
  }
  Set-Acl -LiteralPath $path -AclObject $acl -ErrorAction Stop
  $verified = Get-Acl -LiteralPath $path -ErrorAction Stop
  if (-not $verified.AreAccessRulesProtected) {
    throw "ACL inheritance remains enabled on protected item: $path"
  }
  $entries = @($verified.Access)
  if ($entries.Count -ne 3) {
    throw "Expected exactly three ACL entries on $path; found $($entries.Count)"
  }
  $seenSids = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
  foreach ($entry in $entries) {
    $sid = $entry.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value
    if (-not $seenSids.Add($sid)) {
      throw "Duplicate ACL entry for SID $sid on $path"
    }
    if ($allowed -notcontains $sid -or
        $entry.AccessControlType -ne [System.Security.AccessControl.AccessControlType]::Allow -or
        $entry.FileSystemRights -ne $fullControl -or $entry.IsInherited) {
      throw "Unexpected ACL entry on $path for SID $sid"
    }
    if ($isDirectory -and $entry.InheritanceFlags -ne $inheritance) {
      throw "Directory ACL does not propagate to child files and directories: $path"
    }
  }
  if ($seenSids.Count -ne 3 -or @($allowed | Where-Object { -not $seenSids.Contains($_) }).Count -ne 0) {
    throw "ACL SID set does not exactly match the three authorized SIDs on $path"
  }
}

while ($pending.Count -gt 0) {
  $current = $pending.Pop()
  $item = Get-Item -LiteralPath $current -Force -ErrorAction Stop
  if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "Reparse point found in result storage; refusing to follow it: $current"
  }
  $isDirectory = [bool]$item.PSIsContainer
  Set-RestrictedAcl $current $isDirectory
  if ($isDirectory) {
    foreach ($child in @(Get-ChildItem -LiteralPath $current -Force -ErrorAction Stop)) {
      if (($child.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Reparse point found in result storage; refusing to follow it: $($child.FullName)"
      }
      if ($child.PSIsContainer) {
        $pending.Push($child.FullName)
      } else {
        Set-RestrictedAcl $child.FullName $false
      }
    }
  }
}
Write-Output 'PHONE_RESULT_ACL_OK'
`;

export function secureWindowsResultDirectory(directory, options = {}) {
  if (process.platform !== "win32" && options.platform !== "win32") return;
  if (typeof directory !== "string" || !directory.trim()) {
    throw new TypeError("A result directory is required for Windows ACL protection");
  }
  const runner = options.execFileSync ?? execFileSync;
  const encoded = Buffer.from(POWERSHELL_SCRIPT, "utf16le").toString("base64");
  try {
    const output = runner("powershell.exe", [
      "-NoLogo",
      "-NoProfile",
      "-NonInteractive",
      "-EncodedCommand", encoded
    ], {
      encoding: "utf8",
      windowsHide: true,
      timeout: 60000,
      env: { ...process.env, TOKENSAVER_PHONE_RESULT_DIRECTORY: directory },
      stdio: ["ignore", "pipe", "pipe"]
    });
    if (!String(output).includes("PHONE_RESULT_ACL_OK")) {
      throw new Error("Windows ACL verification marker missing");
    }
  } catch (cause) {
    const detail = cause?.stderr?.toString?.("utf8") || cause?.message || String(cause);
    const code = cause?.code ? " [" + cause.code + "]" : "";
    const error = new Error(
      "Unable to apply and verify restrictive Windows ACLs for Phone-02 result storage" +
      code + ": " + detail.trim().slice(-2000),
      { cause }
    );
    error.code = "PHONE_RESULT_WINDOWS_ACL_FAILED";
    throw error;
  }
}
