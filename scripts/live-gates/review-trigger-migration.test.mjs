// Synthetic credentials and injected fetch only; no provider calls.
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { CREAO_WORKFLOW_CHECK, creaoGate, runGates, scrub } from './run.mjs';
import { runTrigger, triggerRunsUrl } from './creao-trigger.mjs';
const APP='11111111-1111-4111-8111-111111111111', RUN='22222222-2222-4222-8222-222222222222';
const URL=`https://agent.creao.ai/api/v1/apps/${APP}/runs`;
const answer={schema:CREAO_WORKFLOW_CHECK,responsibility_id:'smoke-method-review',owner_role_id:'smoke-design-review',decision:'decline',candidate_successes:2,control_successes:4};
const response=(value,status=200)=>new Response(JSON.stringify(value),{status,headers:{'Content-Type':'application/json'}});
function fixture(t,env={}){
 const out=mkdtempSync(join(tmpdir(),'trigger-migration-'));t.after(()=>rmSync(out,{recursive:true,force:true}));
 const requests=[];
 const opts={repo:resolve(dirname(fileURLToPath(import.meta.url)),'../..'),binDir:out,out,gates:['creao'],stamp:'review',allowBillable:true,pollScale:0,maxSteps:1,
  env:{CREAO_TRIGGER_API_KEY:'capi_review_current',CREAO_TRIGGER_APP_ID:APP,...env},
  triggerFetchImpl:async(url,options)=>{requests.push({url,options});return options.method==='POST'?response({id:RUN,appId:APP,status:'pending'},202):response({id:RUN,status:'completed',result:{text:JSON.stringify(answer)},usage:{credits:0.3}})}};
 return{out,opts,requests,run:extra=>runGates({...opts,...extra})};
}
test('Trigger URL must be exactly the official route for this app',()=>{
 assert.equal(triggerRunsUrl(APP),URL);assert.equal(triggerRunsUrl(APP,URL),URL);
 for(const bad of [`${URL}?x=1`,`${URL}#frag`,URL.replace('https://','https://user:pass@'),URL.replace('agent.creao.ai','agent.creao.ai.evil.example'),URL.replace(APP,RUN),`${URL}/`,URL.replace('/api/','/%61pi/'),URL.replace('https:','http:'),URL.replace('agent.creao.ai','agent.creao.ai:443'),'']) assert.throws(()=>triggerRunsUrl(APP,bad),/exactly match/);
});
test('old account and V2 credentials alone cannot enable the Trigger gate',async t=>{
 const f=fixture(t);const r=await f.run({env:{CREAO_ACCOUNT_API_KEY:'old-account',CREAO_V2_API_KEY:'old-v2'}});
 assert.equal(r.results.creao.status,'not_tested');assert.match(r.results.creao.reason,/CREAO_TRIGGER_API_KEY.*CREAO_TRIGGER_APP_ID/);assert.equal(f.requests.length,0);
});
test('explicit legacy or unknown API versions fail before any Trigger request or journal',async t=>{
 for(const version of ['v1','v2','v3','Trigger',' trigger ']){
  const f=fixture(t,{CREAO_API_VERSION:version});const r=await f.run();
  assert.equal(r.results.creao.status,'failed');assert.equal(r.results.creao.stage,'configuration');assert.match(r.results.creao.reason,/CREAO_API_VERSION must be trigger/);assert.equal(f.requests.length,0);assert.equal(readdirSync(f.out).includes('creao'),false);
 }
});
test('unset, blank and exact Trigger API versions preserve the app-scoped gate',async t=>{
 for(const version of [undefined,'','  ','trigger']){
  const f=fixture(t,{CREAO_API_VERSION:version});assert.equal((await f.run()).results.creao.status,'passed');assert.equal(f.requests.length,2);
 }
});
test('direct and orchestrated preflight do not issue a request',async t=>{
 const f=fixture(t);assert.equal((await f.run({allowBillable:false})).results.creao.status,'not_tested');
 assert.equal((await creaoGate({...f.opts,allowBillable:false})).status,'not_tested');assert.equal(f.requests.length,0);
});
test('invalid key and URL are refused before journal creation',async t=>{
 for(const override of [{CREAO_TRIGGER_API_KEY:'cr_sk_legacy'}, {CREAO_TRIGGER_RUNS_URL:URL+'?creao-api-key=capi_bad'}]){
  const f=fixture(t,override);const r=await f.run();assert.equal(r.results.creao.stage,'configuration');assert.equal(r.results.creao.status,'failed');assert.equal(f.requests.length,0);assert.equal(readdirSync(f.out).includes('creao'),false);
 }
});
test('successful app run retains usage and validates the synthetic receiving decision',async t=>{
 const f=fixture(t,{CREAO_TRIGGER_RUNS_URL:URL,CREAO_ACCOUNT_API_KEY:'cr_sk_unused',CREAO_V2_API_KEY:'cr_sk_unused_v2'});const r=await f.run();
 assert.equal(r.results.creao.status,'passed');assert.equal(r.results.creao.evidence.app_id,APP);assert.equal(r.results.creao.evidence.run_id,RUN);assert.equal(r.results.creao.evidence.credits_used,0.3);assert.equal(r.results.creao.evidence.usage_reason,'reported_by_provider');assert.equal(r.results.creao.evidence.workflow_check,true);assert.equal(r.results.creao.evidence.readiness_verified,false);assert.equal('provider_agent_id' in r.results.creao.evidence,false);
 const post=f.requests[0];assert.equal(post.url,URL);assert.equal(post.options.headers.Authorization,'Bearer capi_review_current');assert.equal(post.options.redirect,'manual');assert.equal(JSON.parse(post.options.body).inputs.local_trial.control.successes,4);
 const journal=JSON.parse(readFileSync(r.results.creao.evidence.checkpoint,'utf8'));assert.deepEqual(journal.result_check,{passed:true});assert.equal(journal.result_check_id,CREAO_WORKFLOW_CHECK);
});
test('native completion with an incorrect workflow decision is failed',async t=>{
 const f=fixture(t);const r=await f.run({triggerFetchImpl:async(_url,options)=>options.method==='POST'?response({id:RUN,appId:APP,status:'pending'},202):response({id:RUN,status:'completed',result:{...answer,decision:'adopt'}})});
 assert.equal(r.results.creao.status,'failed');assert.equal(r.results.creao.evidence.reason,'synthetic_workflow_check_failed');assert.equal(r.results.creao.evidence.workflow_check,false);
});
test('an ambiguous Trigger POST is never repeated by the same gate journal',async t=>{
 const f=fixture(t);let posts=0;const triggerFetchImpl=async()=>{posts++;throw Error('simulated timeout')};
 for(let i=0;i<2;i++){const r=await f.run({triggerFetchImpl});assert.equal(r.results.creao.status,'needs_reconciliation');assert.equal(r.results.creao.evidence.run_id,null);assert.equal(r.results.creao.evidence.reason,'post_outcome_unknown');assert.ok(r.results.creao.evidence.resume_argv.includes('--run-id'));}
 assert.equal(posts,1);
});
test('missing or invalid provider credits are explicitly unknown',async t=>{
 for(const credits of [undefined,-1]){const f=fixture(t);const r=await f.run({triggerFetchImpl:async(_url,options)=>options.method==='POST'?response({id:RUN,appId:APP,status:'pending'},202):response({id:RUN,status:'completed',result:answer,usage:{credits}})});assert.equal(r.results.creao.evidence.credits_used,null);assert.equal(r.results.creao.evidence.usage_reason,'not_reported_by_provider');}
});
test('current, rotated and historical keys cannot survive selected evidence or summaries',async t=>{
 const env={CREAO_TRIGGER_API_KEY:'capi_current_secret',CREAO_TRIGGER_API_KEY_PREVIOUS:'capi_rotated_secret',CREAO_ACCOUNT_API_KEY:'cr_sk_account',CREAO_V2_API_KEY:'cr_sk_v2',CREAO_TRIGGER_API_KEY_OLD:'capi_old_secret'};
 const f=fixture(t,env);const r=await f.run({triggerFetchImpl:async(_url,options)=>options.method==='POST'?response({id:RUN,appId:APP,status:'pending'},202):response({id:RUN,status:'completed',result:{...answer,untrusted_echo:Object.values(env)}})});
 const files=[JSON.stringify(r),...readdirSync(f.out).filter(name=>name.startsWith('gates-')).map(name=>readFileSync(join(f.out,name),'utf8')),readFileSync(r.results.creao.evidence.checkpoint,'utf8')];
 for(const secret of Object.values(env))assert.ok(files.every(body=>!body.includes(secret)));
 const redacted=scrub(Object.values(env),env);assert.ok(redacted.every(value=>value.startsWith('<CREAO_')));
});
test('a journal cannot be resumed with a different output validation contract',async t=>{
 const f=fixture(t);const r=await f.run();
 await assert.rejects(runTrigger({appId:APP,key:'capi_review_current',inputs:(await import('./run.mjs')).creaoWorkflowInputs(),out:r.results.creao.evidence.checkpoint,allowCredits:true,checkId:'another/1',checkResult:()=>true,fetchImpl:()=>{throw Error('network forbidden')}}),/does not match/);
});
