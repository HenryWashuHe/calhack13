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
// Historical Developer API quota-DB guard is superseded by app-run journals.
// Trigger uses account credits reported by CREAO, not the old creation CLI quota DB.
test('separate CREAO Trigger journals retain prior credits and resume without another POST', async () => {
  const app='11111111-1111-4111-8111-111111111111',run='22222222-2222-4222-8222-222222222222';
  const out=mkdtempSync(join(tmpdir(),'trigger-journals-')),binDir=mkdtempSync(join(tmpdir(),'trigger-no-cli-'));
  const response=(value,status=200)=>new Response(JSON.stringify(value),{status,headers:{'Content-Type':'application/json'}});
  let posts=0;
  const opts={repo,binDir,out,env:{CREAO_TRIGGER_API_KEY:'capi_offline',CREAO_TRIGGER_APP_ID:app},gates:['creao'],allowBillable:true,pollScale:0,maxSteps:1,
    triggerFetchImpl:async(_url,options)=>{if(options.method==='POST'){posts++;return response({id:run,appId:app,status:'pending'},202)}return response({id:run,status:'failed',usage:{credits:0.4}})}};
  const first=await runGates({...opts,stamp:'first'}),path=first.results.creao.evidence.checkpoint;
  const original=readFileSync(path,'utf8');
  const second=await runGates({...opts,stamp:'second'});
  assert.notEqual(second.results.creao.evidence.checkpoint,path,'run journals are isolated');
  assert.equal(readFileSync(path,'utf8'),original,'prior evidence is preserved');
  assert.equal(second.results.creao.evidence.app_id,app);
  assert.equal(second.results.creao.evidence.credits_used,0.4);
  await runGates({...opts,stamp:'first'});
  assert.equal(posts,2,'resuming a terminal journal cannot create another run');
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
