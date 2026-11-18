import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const manifest = await readFile(new URL('../../release/phone-02/AppxManifest.template.xml', import.meta.url), 'utf8');
const builder = await readFile(new URL('../../release/phone-02/build-unsigned-msix.ps1', import.meta.url), 'utf8');
const signer = await readFile(new URL('../../release/phone-02/sign-msix.ps1', import.meta.url), 'utf8');
const workflow = await readFile(new URL('../../.github/workflows/phone-02-release-package.yml', import.meta.url), 'utf8');

test('approved Publisher defaults preserve package identity and x64 architecture', () => {
  assert.ok(manifest.includes('Name="TokenSaver.Phone02.Supervisor"'));
  assert.ok(manifest.includes('ProcessorArchitecture="x64"'));
  assert.ok(manifest.includes('Publisher="__PUBLISHER__"'));
  assert.ok(manifest.includes('PublisherDisplayName>__PUBLISHER_DISPLAY_NAME__<'));
  assert.ok(builder.includes("Publisher = 'CN=TokenSaver Publisher'"));
  assert.ok(builder.includes("PublisherDisplayName = 'TokenSaver AI'"));
  assert.ok(workflow.includes("$publisher = 'CN=TokenSaver Publisher'"));
  assert.ok(workflow.includes("$displayName = 'TokenSaver AI'"));
  assert.ok(!builder.includes('PENDING APPROVAL'));
  assert.ok(!workflow.includes('PENDING APPROVAL'));
});

test('manifest retains Windows desktop target and existing runtime architecture', () => {
  assert.ok(manifest.includes('TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0"'));
  assert.ok(manifest.includes('Executable="Phone02Client.exe" uap10:TrustLevel="appContainer"'));
  assert.ok(manifest.includes('Executable="SupervisorHost.exe" EntryPoint="Windows.FullTrustApplication"'));
  assert.ok(manifest.includes('Category="windows.fullTrustProcess" Executable="Supervisor.exe"'));
  assert.ok(manifest.includes('Capability Name="runFullTrust"'));
  assert.doesNotMatch(manifest, /TEST ONLY|SupervisorTest|P236|P238|fixture|fake/i);
});

test('unsigned build enforces source integrity, identity checks and provenance', () => {
  for (const marker of ['ExpectedSourceSha', 'SOURCE_SHA_EXACT', 'WORKTREE_CLEAN', 'PACKAGE_IDENTITY',
    'PACKAGE_ARCH_X64', 'PUBLISHER_MANIFEST_MATCH', 'PUBLISHER_DISPLAY_NAME_MATCH', 'MIDL_GENERATION',
    'SUPERVISOR_COMPILE', 'CLIENT_COMPILE', 'MAKEAPPX_PACK', 'MAKEAPPX_VALIDATE', 'provenance.json',
    "signature_status='UNSIGNED'", "approval_status='PENDING'']) {
    assert.ok(builder.includes(marker), 'missing builder marker: ' + marker);
  }
  assert.ok(builder.includes('Get-FileHash'));
});

test('signing script requires exact approved Publisher and separately approved certificate fingerprint', () => {
  for (const marker of ['ExpectedPublisher', 'ExpectedPublisherDisplayName', 'ApprovedThumbprint',
    'SecurityApprovalSecret', 'Certificate subject does not match approved Publisher',
    'Certificate fingerprint does not match separately approved fingerprint',
    'SIGNATURE_VALID=PASS', 'Timestamp verification evidence missing']) {
    assert.ok(signer.includes(marker), 'missing signing guard: ' + marker);
  }
});

test('release workflow is unsigned-only, Windows x64 and read-only', () => {
  assert.ok(workflow.includes('runs-on: windows-2025'));
  assert.ok(workflow.includes('ref: ${{ github.sha }}'));
  assert.ok(workflow.includes('ExpectedSourceSha $env:GITHUB_SHA'));
  assert.ok(workflow.includes('contents: read'));
  assert.ok(workflow.includes('phone02-release-unsigned-'));
  assert.doesNotMatch(workflow, /MSIX_CERTIFICATE_BASE64|MSIX_SECURITY_RELEASE_APPROVED|sign-approved:|id-token: write/);
});
