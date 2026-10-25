// Inert CLI fixture: no network or provider credentials.
import assert from 'node:assert/strict';
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { elasticGate } from './run.mjs';

test('Elastic smoke IDs are short and include the whole run identity', async t => {
  const root = mkdtempSync(join(tmpdir(), 'review263-'));
  t.after(() => rmSync(root, {recursive:true, force:true}));
  const cli = join(root, 'genesis-elastic-smoke');
  writeFileSync(cli, `#!/usr/bin/env node
const fs = require('node:fs'); const a=process.argv.slice(2);
fs.writeFileSync(a[a.indexOf('--report')+1], JSON.stringify({ok:true,org:a[a.indexOf('--org')+1],steps:[]}));
`);
  chmodSync(cli, 0o755);
  const orgs=[];
  for (const stamp of ['20261025103959','20261026103959','alice-shared','bravo-shared','x'.repeat(128)]) {
    const report = await elasticGate({binDir:root,out:root,stamp,env:{PATH:process.env.PATH}});
    assert.equal(report.status, 'passed');
    const {org}=JSON.parse(readFileSync(join(root, 'elastic-smoke.json'), 'utf8'));
    assert.match(org,/^[a-z0-9-]+$/);
    // Matches the Rust adapter's prefix + org_namespace + purpose suffix.
    assert.ok(`genesis-${org}-${'0'.repeat(16)}-evidence-search`.length <= 63);
    orgs.push(org);
  }
  assert.equal(new Set(orgs).size,orgs.length,'different dates/custom IDs must not alias on their suffix');
  await elasticGate({binDir:root,out:root,stamp:'20261025103959',env:{PATH:process.env.PATH}});
  assert.equal(JSON.parse(readFileSync(join(root,'elastic-smoke.json'),'utf8')).org,orgs[0]);
});
