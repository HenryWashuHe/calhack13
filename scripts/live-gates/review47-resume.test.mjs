// Follow-up review of PR47 at 0aa2169. No network or provider credentials.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { runGates } from './run.mjs';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
function fixture(name, script, spaced = false) {
  const dir = mkdtempSync(join(tmpdir(), 'genesis-review47-followup-'));
  const binDir = join(dir, 'bin');
  const out = join(dir, spaced ? 'output with spaces' : 'output');
  const log = join(dir, 'calls.jsonl');
  const executable = join(binDir, name);
  const env = {PATH: `${binDir}:${process.env.PATH}`, APIFY_TOKEN: 'offline-only-token', CREAO_ACCOUNT_API_KEY: 'offline-only-token'};
  mkdirSync(binDir);
  writeFileSync(executable, `#!/usr/bin/env node\nconst fs = require('node:fs'); const args = process.argv.slice(2);\nfs.appendFileSync(${JSON.stringify(log)}, JSON.stringify(args)+'\\n');\n${script}\n`);
  chmodSync(executable, 0o755);
  return {
    run: (stamp='first') => runGates({repo, binDir, out, env, gates:[name==='genesis-forge'?'apify':'creao'],stamp,allowBillable:true,pollScale:0,maxSteps:1}),
    calls: () => readFileSync(log, 'utf8').trim().split('\n').map(JSON.parse), env,
  };
}
test('separate CREAO operation directories retain one account quota database', async () => {
  const f=fixture('genesis-creao', `if(args[0]==='prepare') process.exit(0); console.log(JSON.stringify({phase:'native_completed',run_id:'r',provider_agent_id:'a'}));`);
  await f.run('first'); await f.run('second');
  const steps=f.calls().filter(args=>args[0]==='step');
  const arg=(args,name)=>args[args.indexOf(name)+1];
  assert.notEqual(arg(steps[0],'--db'),arg(steps[1],'--db'),'operation checkpoints are isolated');
  assert.equal(arg(steps[0],'--account'),arg(steps[1],'--account'));
  assert.equal(arg(steps[0],'--quota-db'),arg(steps[1],'--quota-db'),'the same account must retain its request/write debits and cooldown between runs');
});
test('a retryable read at the step limit emits an executable retry resume command', async () => {
  const f=fixture('genesis-forge', `if(args[0]==='prepare') process.exit(0);
const marker=args[args.indexOf('--checkpoint')+1]+'.retry-marker';
if(!fs.existsSync(marker)){fs.writeFileSync(marker,'read failed');console.log(JSON.stringify({phase:'build_submitted',failure:{retryable:true,ambiguous:false,build_id:'build-1'}}));process.exit(1);}
if(!args.includes('--retry-read')) { console.error('previous read failed; explicitly use --retry-read'); process.exit(2); }
console.log(JSON.stringify({phase:'verified'}));`);
  const report=await f.run();
  assert.equal(report.results.apify.status,'partial');
  const resumed=spawnSync('/bin/sh',['-c',report.results.apify.evidence.resume],{env:f.env,encoding:'utf8'});
  assert.equal(resumed.status,0,resumed.stderr);
});
test('resume commands preserve an output directory containing spaces', async () => {
  const f=fixture('genesis-forge', `if(args[0]==='prepare') process.exit(0);
const allowed=new Set(['step','--suite','--checkpoint']);
for(let i=0;i<args.length;i++){if(args[i]==='--suite'||args[i]==='--checkpoint'){i++;continue;}if(!allowed.has(args[i])){console.error('unexpected argument: '+args[i]);process.exit(2);}}
console.log(JSON.stringify({phase:'testing',artifact:{build_id:'build-1'}}));`, true);
  const report=await f.run();
  const resumed=spawnSync('/bin/sh',['-c',report.results.apify.evidence.resume],{env:f.env,encoding:'utf8'});
  assert.equal(resumed.status,0,resumed.stderr);
});
