import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
const root = new URL('../../', import.meta.url);
const read = p => readFile(new URL(p, root), 'utf8');
const [manifest,builder,signer,workflow] = await Promise.all([
  read('release/phone-02/AppxManifest.template.xml'),
  read('release/phone-02/build-unsigned-msix.ps1'),
  read('release/phone-02/sign-msix.ps1'),
  read('.github/workflows/phone-02-release-package.yml')
]);
test('approved Publisher and package identity remain fixed',()=>{
  assert.ok(manifest.includes('Name="TokenSaver.Phone02.Supervisor"'));
  assert.ok(manifest.includes('ProcessorArchitecture="x64"'));
  assert.ok(builder.includes("Publisher = 'CN=TokenSaver Publisher'"));
  assert.ok(builder.includes("PublisherDisplayName = 'TokenSaver AI'"));
});
test('unsigned build integrity and provenance gates remain intact',()=>{
  for(const marker of ['ExpectedSourceSha','SOURCE_SHA_EXACT','WORKTREE_CLEAN','PACKAGE_IDENTITY','PACKAGE_ARCH_X64','PUBLISHER_MANIFEST_MATCH','MAKEAPPX_PACK','MAKEAPPX_VALIDATE','provenance.json',"signature_status='UNSIGNED'"]) assert.ok(builder.includes(marker),marker);
});
test('signing uses certificate store and has no password/PFX path',()=>{
  for(const marker of ['ApprovedThumbprint','CERTIFICATE_PENDING','SIGNING_BLOCKED','HasPrivateKey','X509Chain','1.3.6.1.5.5.7.3.3','Cert:\\LocalMachine\\My','/sha1']) assert.ok(signer.includes(marker),marker);
  assert.ok(!signer.includes('CertificatePassword'));
  assert.ok(!signer.includes('CertificatePath'));
  assert.ok(!signer.includes('SecurityApprovalSecret'));
  assert.ok(!signer.includes('/p $'));
});
test('workflow separates build/sign/verify and requires explicit opt-in',()=>{
  assert.ok(workflow.includes('build-unsigned:'));
  assert.ok(workflow.includes('enable_signing:'));
  assert.ok(workflow.includes('default: false'));
  assert.ok(workflow.includes("inputs.enable_signing == true"));
  assert.ok(workflow.includes('name: phone-02-signing'));
  assert.ok(workflow.includes('verify-signature:'));
  assert.ok(workflow.includes('release-evidence:'));
  const build=workflow.split('  signing-infrastructure-tests:')[0];
  assert.ok(!build.includes('PHONE02_SIGNING_CERT_THUMBPRINT'));
  assert.ok(!build.includes('secrets.'));
});
