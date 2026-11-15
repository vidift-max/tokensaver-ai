import { execFileSync } from "node:child_process";

const POWERSHELL_SCRIPT = String.raw`
$ErrorActionPreference = 'Stop'
$target = [System.IO.Path]::GetFullPath($env:TOKENSAVER_PHONE_RESULT_DIRECTORY)
if (-not (Test-Path -LiteralPath $target -PathType Container)) {
  throw 'Phone result directory does not exist'
}
$currentSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$allowed = @($currentSid, 'S-1-5-18', 'S-1-5-32-544')
$inheritance = [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [System.Security.AccessControl.InheritanceFlags]::ObjectInherit

function Set-RestrictedAcl([string]$path, [bool]$isDirectory) {
  $acl = Get-Acl -LiteralPath $path
  $acl.SetAccessRuleProtection($true, $false)
  foreach ($existing in @($acl.Access)) {
    $acl.RemoveAccessRuleAll($existing)
  }
  foreach ($sid in $allowed) {
    $identity = [System.Security.Principal.SecurityIdentifier]::new($sid)
    if ($isDirectory) {
      $rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
        $identity,
        [System.Security.AccessControl.FileSystemRights]::FullControl,
        $inheritance,
        [System.Security.AccessControl.PropagationFlags]::None,
        [System.Security.AccessControl.AccessControlType]::Allow
      )
    } else {
      $rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
        $identity,
        [System.Security.AccessControl.FileSystemRights]::FullControl,
        [System.Security.AccessControl.AccessControlType]::Allow
      )
    }
    $acl.AddAccessRule($rule)
  }
  Set-Acl -LiteralPath $path -AclObject $acl
  $verified = Get-Acl -LiteralPath $path
  if (-not $verified.AreAccessRulesProtected) {
    throw "ACL inheritance remains enabled: $path"
  }
  foreach ($entry in $verified.Access) {
    $sid = $entry.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value
    if ($allowed -notcontains $sid -or $entry.AccessControlType -ne 'Allow' -or
        $entry.FileSystemRights -ne 'FullControl' -or $entry.IsInherited) {
      throw "Unexpected or inherited ACL entry on $path for $sid"
    }
  }
  if (@($verified.Access).Count -ne 3) {
    throw "Expected exactly three restricted ACL entries on $path"
  }
}

Set-RestrictedAcl $target $true
Get-ChildItem -LiteralPath $target -Force -File | ForEach-Object {
  Set-RestrictedAcl $_.FullName $false
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
      "-ExecutionPolicy", "AllSigned",
      "-EncodedCommand", encoded
    ], {
      encoding: "utf8",
      windowsHide: true,
      timeout: 15000,
      env: { ...process.env, TOKENSAVER_PHONE_RESULT_DIRECTORY: directory },
      stdio: ["ignore", "pipe", "pipe"]
    });
    if (!String(output).includes("PHONE_RESULT_ACL_OK")) {
      throw new Error("Windows ACL verification marker missing");
    }
  } catch (cause) {
    const error = new Error("Unable to apply and verify restrictive Windows ACLs for Phone-02 result storage", { cause });
    error.code = "PHONE_RESULT_WINDOWS_ACL_FAILED";
    throw error;
  }
}
