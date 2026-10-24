// Review regressions for PR47 at 339e90c. No provider calls or credentials.
// node --test scripts/live-gates/review47.test.mjs
import assert from 'node:assert/strict';
import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { runGates } from './run.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const env = { PATH: process.env.PATH, APIFY_TOKEN: 'offline-review-token', CREAO_ACCOUNT_API_KEY: 'offline-review-token' };
function setup(name, script) {
  const binDir = mkdtempSync(join(tmpdir(), 'genesis-review47-bin-'));
  const out = mkdtempSync(join(tmpdir(), 'genesis-review47-out-'));
  const executable = join(binDir, name);
  const calls = join(binDir, 'calls.jsonl');
  writeFileSync(executable, `#!/usr/bin/env node\nconst fs = require('node:fs');\nconst args = process.argv.slice(2);\nfs.appendFileSync(${JSON.stringify(calls)}, JSON.stringify(args) + '\\n');\n${script}\n`);
  chmodSync(executable, 0o755);
  return {
    run: (stamp = 'first') => runGates({ repo, binDir, out, gates: [name === 'genesis-forge' ? 'apify' : 'creao'], env, stamp, pollScale: 0, allowBillable: true, maxSteps: 8 }),
    calls: () => readFileSync(calls, 'utf8').trim().split('\n').map(JSON.parse),
    out,
  };
}

test('retryable Forge GET failures must reach the explicit read retry', async () => {
  // Actual genesis-forge step emits this nonterminal failure JSON and exits 1:
  // crates/genesis-forge/src/main.rs:284-305, session.rs:402-410.
  const fixture = setup('genesis-forge', `
if (args[0] === 'prepare') process.exit(0);
if (!args.includes('--retry-read')) {
  console.log(JSON.stringify({ phase: 'build_submitted', pending_action: null,
    failure: { retryable: true, ambiguous: false, actor_id: 'actor-1', build_id: 'build-1', run_id: null } }));
  process.exit(1);
}
console.log(JSON.stringify({ phase: 'verified', artifact: { actor_id: 'actor-1', build_id: 'build-1' } }));`);
  const report = await fixture.run();
  assert.equal(fixture.calls().filter(args => args[0] === 'step').length, 2,
    'the first recoverable read error must not end the gate before --retry-read');
  assert.equal(report.results.apify.status, 'passed');
});

test('a native CREAO failure remains failed after the run was created', async () => {
  // Runtime::step returns a stored failed record and the real CLI exits 0.
  // crates/genesis-creao/src/runtime.rs:605-614 and main.rs:132-152.
  const fixture = setup('genesis-creao', `
if (args[0] === 'prepare') process.exit(0);
console.log(JSON.stringify({ phase: 'failed', native_status: 'failed', run_id: 'run-failed',
  provider_agent_id: null, retryable_read: false, failure: 'native_failed' }));`);
  const report = await fixture.run();
  assert.equal(report.results.creao.status, 'failed',
    'a terminal failed phase must cause the runner exit policy to fail');
});

test('an oracle-rejected Apify run remains failed after its build succeeded', async () => {
  // Forge exits 1 for rejected, but the gate currently treats build_id as partial.
  const fixture = setup('genesis-forge', `
if (args[0] === 'prepare') process.exit(0);
console.log(JSON.stringify({ phase: 'rejected', completed_cases: 3, total_cases: 3,
  artifact: { actor_id: 'actor-1', build_id: 'build-1', build_number: '0.0.1' } }));
process.exit(1);`);
  const report = await fixture.run();
  assert.equal(report.results.apify.status, 'failed',
    'an oracle rejection is a failed verification gate, not merely incomplete evidence');
});

test('a second gate run preserves the first checkpoint and can prepare its own operation', async () => {
  // Mirrors CheckpointLock::create: prepare refuses an existing checkpoint.
  const fixture = setup('genesis-forge', `
if (args[0] === 'prepare') {
  const checkpoint = args[args.indexOf('--checkpoint') + 1];
  const candidate = JSON.parse(fs.readFileSync(args[args.indexOf('--candidate') + 1], 'utf8'));
  try { fs.writeFileSync(checkpoint, JSON.stringify(candidate), { flag: 'wx' }); }
  catch { console.error('checkpoint already exists; prepare never overwrites it'); process.exit(2); }
  process.exit(0);
}
console.log(JSON.stringify({ phase: 'verified', artifact: { actor_id: 'actor-1', build_id: 'build-1' } }));`);
  assert.equal((await fixture.run('first')).results.apify.status, 'passed');
  const prepareArgs = fixture.calls().find(args => args[0] === 'prepare');
  const checkpointPath = prepareArgs[prepareArgs.indexOf('--checkpoint') + 1];
  const originalCheckpoint = readFileSync(checkpointPath, 'utf8');
  const second = await fixture.run('second');
  assert.equal(readFileSync(checkpointPath, 'utf8'), originalCheckpoint,
    'prior evidence is immutable');
  assert.equal(second.results.apify.status, 'passed',
    'separate stamps must not collide on the fixed apify/forge.json checkpoint');
});
