import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const manifest = await readFile(new URL('../../release/phone-02/AppxManifest.template.xml', import.meta.url), 'utf8');
const builder = await readFile(new URL('../../release/phone-02/build-unsigned-msix.ps1', import.meta.url), 'utf8');
const signer = await readFile(new URL('../../release/phone-02/sign-msix.ps1', import.meta.url), 'utf8');
const workflow = await readFile(new URL('../../.github/workflows/phone-02-release-package.yml', import.meta.url), 'utf8');

test('production candidate package identity, target and architecture are declared', () => {
  assert.match(manifest, /Name="TokenSaver\.Phone02\.Supervisor"/);
  assert.match(manifest, /ProcessorArchitecture="x64"/);
  assert.match(manifest, /TargetDeviceFamily Name="Windows\.Desktop" MinVersion="10\.0\.19041\.0"/);
  assert.match(manifest, /Publisher="__PUBLISHER__"/);
  assert.match(manifest, /Version="__VERSION__"/);
});

test('AppContainer client and FullTrust Supervisor extension are explicit', () => {
  assert.match(manifest, /Executable="Phone02Client\.exe"[^>]*uap10:TrustLevel="appContainer"/);
  assert.match(manifest, /Executable="SupervisorHost\.exe" EntryPoint="Windows\.FullTrustApplication"/);
  assert.match(manifest, /Category="windows\.fullTrustProcess" Executable="Supervisor\.exe"/);
  assert.match(manifest, /Capability Name="runFullTrust"/);
});

test('production manifest contains no test identities, publishers or fixture markers', () => {
  assert.doesNotMatch(manifest, /TEST ONLY|SupervisorTest|P236|P238|fixture|fake/i);
  assert.match(manifest, /PublisherDisplayName>__PUBLISHER_DISPLAY_NAME__</);
  assert.match(manifest, /PublisherDisplayName/);
});

test('build script requires exact source SHA and clean tree, then hashes provenance', () => {
  assert.match(builder, /ExpectedSourceSha/);
  assert.match(builder, /SOURCE_SHA_EXACT/);
  assert.match(builder, /WORKTREE_CLEAN/);
  assert.match(builder, /MIDL_GENERATION/);
  assert.match(builder, /SUPERVISOR_COMPILE/);
  assert.match(builder, /CLIENT_COMPILE/);
  assert.match(builder, /MAKEAPPX_PACK/);
  assert.match(builder, /MAKEAPPX_VALIDATE/);
  assert.match(builder, /\/MT/);
  assert.match(builder, /Get-FileHash .*SHA256/);
  assert.match(builder, /provenance\.json/);
  assert.match(builder, /BYTE_IDENTICAL_REBUILD_NOT_VERIFIED/);
  assert.match(builder, /SIGNING READY — CERTIFICATE\/SIGNER PENDING/);
});

test('signing script refuses pending publisher and verifies approved signer fingerprint', () => {
  assert.match(signer, /PENDING APPROVAL\|TEST ONLY/);
  assert.match(signer, /ExpectedPublisher/);
  assert.match(signer, /ApprovedThumbprint/);
  assert.match(signer, /SecurityApprovalSecret/);
  assert.match(signer, /unsigned_msix_sha256/);
  assert.match(signer, /Package architecture mismatch/);
  assert.match(signer, /explicit Security approval secret is missing or invalid/);
  assert.match(signer, /\bsign\b/i);
  assert.match(signer, /\bverify\b/i);
  assert.match(signer, /SIGNATURE_VALID=PASS/);
  assert.match(signer, /Timestamp verification evidence missing/);
  assert.match(signer, /HashManifestPath/);
});

test('release workflow uses Windows runner, exact SHA and conditional signing secrets', () => {
  assert.match(workflow, /runs-on: windows-2025/);
  assert.match(workflow, /ref: \$\{\{ github\.sha \}\}/);
  assert.match(workflow, /ExpectedSourceSha \$env:GITHUB_SHA/);
  assert.match(workflow, /MSIX_CERTIFICATE_BASE64/);
  assert.match(workflow, /security_approval == true/);
  assert.match(workflow, /publisher_display_name != ''/);
  assert.match(workflow, /environment: phone02-release-security/);
  assert.match(workflow, /MSIX_APPROVED_THUMBPRINT/);
  assert.match(workflow, /MSIX_SECURITY_RELEASE_APPROVED/);
  assert.match(workflow, /sign-approved:/);
  assert.match(workflow, /phone02-release-unsigned/);
  assert.match(workflow, /phone02-release-signed/);
});
