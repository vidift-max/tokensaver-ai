import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const manifest = await readFile(new URL('../../release/phone-02/AppxManifest.template.xml', import.meta.url), 'utf8');
const builder = await readFile(new URL('../../release/phone-02/build-unsigned-msix.ps1', import.meta.url), 'utf8');
const signer = await readFile(new URL('../../release/phone-02/sign-msix.ps1', import.meta.url), 'utf8');
const workflow = await readFile(new URL('../../.github/workflows/phone-02-release-package.yml', import.meta.url), 'utf8');

test('approved Publisher defaults preserve package identity and x64 architecture', () => {
  assert.match(manifest, /Name="TokenSaver\\.Phone02\\.Supervisor"/);
  assert.match(manifest, /ProcessorArchitecture="x64"/);
  assert.match(manifest, /Publisher="__PUBLISHER__"/);
  assert.match(manifest, /PublisherDisplayName>__PUBLISHER_DISPLAY_NAME__</);
  assert.match(builder, /Publisher = 'CN=TokenSaver Publisher'/);
  assert.match(builder, /PublisherDisplayName = 'TokenSaver AI'/);
  assert.match(workflow, /\\$publisher = 'CN=TokenSaver Publisher'/);
  assert.match(workflow, /\\$displayName = 'TokenSaver AI'/);
  assert.doesNotMatch(builder, /PENDING APPROVAL/);
  assert.doesNotMatch(workflow, /PENDING APPROVAL/);
});

test('manifest retains Windows desktop target and existing runtime architecture', () => {
  assert.match(manifest, /TargetDeviceFamily Name="Windows\\.Desktop" MinVersion="10\\.0\\.19041\\.0"/);
  assert.match(manifest, /Executable="Phone02Client\\.exe"[^>]*uap10:TrustLevel="appContainer"/);
  assert.match(manifest, /Executable="SupervisorHost\\.exe" EntryPoint="Windows\\.FullTrustApplication"/);
  assert.match(manifest, /Category="windows\\.fullTrustProcess" Executable="Supervisor\\.exe"/);
  assert.match(manifest, /Capability Name="runFullTrust"/);
  assert.doesNotMatch(manifest, /TEST ONLY|SupervisorTest|P236|P238|fixture|fake/i);
});

test('unsigned build still enforces source integrity and records provenance', () => {
  assert.match(builder, /ExpectedSourceSha/);
  assert.match(builder, /SOURCE_SHA_EXACT/);
  assert.match(builder, /WORKTREE_CLEAN/);
  assert.match(builder, /PACKAGE_IDENTITY/);
  assert.match(builder, /PACKAGE_ARCH_X64/);
  assert.match(builder, /PUBLISHER_MANIFEST_MATCH/);
  assert.match(builder, /PUBLISHER_DISPLAY_NAME_MATCH/);
  assert.match(builder, /MIDL_GENERATION/);
  assert.match(builder, /SUPERVISOR_COMPILE/);
  assert.match(builder, /CLIENT_COMPILE/);
  assert.match(builder, /MAKEAPPX_PACK/);
  assert.match(builder, /MAKEAPPX_VALIDATE/);
  assert.match(builder, /Get-FileHash .*SHA256/);
  assert.match(builder, /provenance\\.json/);
  assert.match(builder, /signature_status='UNSIGNED'/);
  assert.match(builder, /approval_status='PENDING'/);
});

test('signing script requires the exact approved Publisher and separately approved certificate fingerprint', () => {
  assert.match(signer, /ExpectedPublisher/);
  assert.match(signer, /ExpectedPublisherDisplayName/);
  assert.match(signer, /ApprovedThumbprint/);
  assert.match(signer, /SecurityApprovalSecret/);
  assert.match(signer, /Certificate subject does not match approved Publisher/);
  assert.match(signer, /Certificate fingerprint does not match separately approved fingerprint/);
  assert.match(signer, /SIGNATURE_VALID=PASS/);
  assert.match(signer, /Timestamp verification evidence missing/);
});

test('release workflow builds unsigned on Windows from exact SHA with read-only repository permission', () => {
  assert.match(workflow, /runs-on: windows-2025/);
  assert.match(workflow, /ref: \\$\\{\\{ github\\.sha \\}\\}/);
  assert.match(workflow, /ExpectedSourceSha \\$env:GITHUB_SHA/);
  assert.match(workflow, /permissions:\\s+contents: read/);
  assert.match(workflow, /phone02-release-unsigned-/);
  assert.doesNotMatch(workflow, /MSIX_CERTIFICATE_BASE64|MSIX_SECURITY_RELEASE_APPROVED|sign-approved:|id-token: write/);
});
