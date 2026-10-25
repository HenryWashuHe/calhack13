// node --test scripts/live-gates/gates.test.mjs
// Orchestration tests with stub CLIs: no network, no credentials.
import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { CREAO_WORKFLOW_CHECK, runGates, scrub } from "./run.mjs";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const APP = "11111111-1111-4111-8111-111111111111";
const RUN = "22222222-2222-4222-8222-222222222222";
const answer = {schema: CREAO_WORKFLOW_CHECK, responsibility_id:"smoke-method-review", owner_role_id:"smoke-design-review", decision:"decline", candidate_successes:2, control_successes:4};
const response = (value, status=200) => new Response(JSON.stringify(value), {status, headers:{"Content-Type":"application/json"}});
const completeTrigger = async (_url, options) => options.method === "POST" ? response({id:RUN,appId:APP,status:"pending"},202) : response({id:RUN,status:"completed",result:{text:JSON.stringify(answer)},usage:{credits:0.2}});
const SECRET = "sk-live-SUPERSECRET-123456";

/** A stub executable: `script` is JS run with argv/env; state in STUB_STATE. */
function stub(dir, name, script) {
  const path = join(dir, name);
  writeFileSync(
    path,
    `#!/usr/bin/env node
const fs = require("fs"); const args = process.argv.slice(2);
const stateFile = ${JSON.stringify(join(dir, `${name}.state`))};
let n = 0; try { n = Number(fs.readFileSync(stateFile, "utf8")); } catch {}
const calls = ${JSON.stringify(join(dir, `${name}.calls`))};
fs.appendFileSync(calls, args.join(" ") + "\\n");
${script}
`,
  );
  chmodSync(path, 0o755);
  return path;
}
const calls = (dir, name) => {
  try {
    return readFileSync(join(dir, `${name}.calls`), "utf8").trim().split("\n");
  } catch {
    return [];
  }
};
const fullEnv = {
  PATH: process.env.PATH,
  ELASTICSEARCH_URL: "https://es.example",
  ELASTIC_DATA_API_KEY: SECRET,
  ELASTIC_KIBANA_URL: "https://kb.example",
  ELASTIC_MANAGEMENT_API_KEY: SECRET + "m",
  AGENTVERSE_API_KEY: SECRET + "a",
  APIFY_TOKEN: SECRET + "p",
  CREAO_ACCOUNT_API_KEY: SECRET + "c", // Historical credentials remain scrubbed, never used for this gate.
  CREAO_TRIGGER_API_KEY: "capi_offline_fixture_key",
  CREAO_TRIGGER_APP_ID: APP,
};
const run = (bin, out, extra = {}) =>
  runGates({ repo: REPO, binDir: bin, out, gates: ["elastic", "fetch", "apify", "creao"], env: fullEnv, stamp: "t1", pollScale: 0, allowBillable: true, triggerFetchImpl: async () => { throw new Error("network forbidden in tests"); }, ...extra });

test("missing credentials or binaries are not_tested, never a pass", async () => {
  const bin = mkdtempSync(join(tmpdir(), "bin-"));
  const out = mkdtempSync(join(tmpdir(), "out-"));
  const r = await runGates({ repo: REPO, binDir: bin, out, gates: ["elastic", "fetch", "apify", "creao"], env: { PATH: process.env.PATH }, stamp: "t0" });
  for (const g of Object.values(r.results)) {
    assert.equal(g.status, "not_tested", g.gate);
    assert.match(g.reason, /missing/);
  }
  const noBins = await run(bin, out, {gates:["elastic","fetch","apify"]});
  for (const g of Object.values(noBins.results)) assert.match(g.reason, /^no /, g.gate);
});

test("billable gates need explicit consent", async () => {
  const bin = mkdtempSync(join(tmpdir(), "bin-"));
  stub(bin, "genesis-creao", `process.exit(9)`);
  const r = await run(bin, mkdtempSync(join(tmpdir(), "out-")), { gates: ["apify", "creao"], allowBillable: false });
  assert.match(r.results.creao.reason, /allow-billable/);
  assert.match(r.results.apify.reason, /allow-billable/);
  assert.deepEqual(calls(bin, "genesis-creao"), [], "no billable call without consent");
});

test("elastic and fetch evidence is parsed and secrets are scrubbed", async () => {
  const bin = mkdtempSync(join(tmpdir(), "bin-"));
  const out = mkdtempSync(join(tmpdir(), "out-"));
  stub(
    bin,
    "genesis-elastic-smoke",
    `const report = args[args.indexOf("--report") + 1];
fs.writeFileSync(report, JSON.stringify({ org: "x", ok: true, steps: [
  { step: "create specialist", ok: true, detail: { Ok: "genesis-x-agent" } },
  { step: "converse uses evidence tool", ok: true, detail: { conversation_id: "conv-1", status: "completed", leaked: process.env.ELASTIC_DATA_API_KEY } }]}));
console.log("[PASS] all");`,
  );
  stub(bin, "genesis-fetch", `console.log(JSON.stringify({ search_id: "s-1", num_hits: 3, total: 3, candidates: [], rejected_candidates: 3 }))`);
  const r = await run(bin, out, { gates: ["elastic", "fetch"] });
  assert.equal(r.results.elastic.status, "passed");
  assert.equal(r.results.elastic.evidence.agent_id, "genesis-x-agent");
  assert.equal(r.results.elastic.evidence.conversation_id, "conv-1");
  assert.equal(r.results.fetch.status, "partial", "discovery alone is not the full Fetch gate");
  assert.equal(r.results.fetch.evidence.search_id, "s-1");
  for (const f of readdirSync(out).filter((f) => f.startsWith("gates-"))) {
    assert.ok(!readFileSync(join(out, f), "utf8").includes(SECRET), `${f} contains a credential`);
  }
  assert.equal(scrub(`key=${SECRET}`, fullEnv), "key=<ELASTIC_DATA_API_KEY>");
});

// Historical Developer API agent-creation/V2 assertions below were replaced
// deliberately by the credit-consuming app-run contract. No creation CLI may run.
test("creao Trigger completion records the existing app and retries only GET with the same journal", async () => {
  const bin = mkdtempSync(join(tmpdir(), "bin-"));
  stub(bin, "genesis-creao", `throw Error("agent creation must not run")`);
  const out = mkdtempSync(join(tmpdir(), "out-")), requests=[];
  let gets=0;
  const triggerFetchImpl=async(url,options)=>{
    requests.push({url,method:options.method});
    if(options.method==="POST") return response({id:RUN,appId:APP,status:"pending"},202);
    gets++;
    if(gets===1) return response({},500);
    return response({id:RUN,status:gets===2?"running":"completed",result:answer,usage:{credits:0.2}});
  };
  const first=await run(bin,out,{gates:["creao"],triggerFetchImpl});
  assert.equal(first.results.creao.status,"partial");
  const r=await run(bin,out,{gates:["creao"],triggerFetchImpl});
  assert.equal(r.results.creao.status,"passed");
  assert.equal(r.results.creao.evidence.app_id,APP);
  assert.equal(r.results.creao.evidence.run_id,RUN);
  assert.equal(r.results.creao.evidence.api_version,"trigger");
  assert.equal(r.results.creao.evidence.readiness_verified,false);
  assert.equal(requests.filter(r=>r.method==="POST").length,1);
  assert.equal(requests.filter(r=>r.method==="GET").length,3);
  assert.deepEqual(calls(bin,"genesis-creao"),[]);
});

// Legacy v2 used to select the developer endpoint. Only explicit Trigger now
// permits a request; retain the positive wire-shape assertions on that contract.
test("creao explicit Trigger setting uses only the app-scoped request contract", async () => {
  const bin=mkdtempSync(join(tmpdir(),"bin-")),out=mkdtempSync(join(tmpdir(),"out-"));
  stub(bin,"genesis-creao",`process.exit(1)`);
  const requests=[];
  const r=await run(bin,out,{gates:["creao"],env:{...fullEnv,CREAO_API_VERSION:"trigger"},triggerFetchImpl:async(url,options)=>{requests.push({url,options});return completeTrigger(url,options)}});
  assert.equal(r.results.creao.status,"passed");
  assert.equal(requests[0].url,`https://agent.creao.ai/api/v1/apps/${APP}/runs`);
  const request=JSON.parse(requests[0].options.body);
  assert.deepEqual(Object.keys(request),["inputs"]);
  assert.equal(request.inputs.schema,CREAO_WORKFLOW_CHECK);
  assert.deepEqual(calls(bin,"genesis-creao"),[]);
});

test("creao Trigger labels public evidence and refuses a wrong-app URL before a request", async () => {
  const bin=mkdtempSync(join(tmpdir(),"bin-")),out=mkdtempSync(join(tmpdir(),"out-"));
  const hosted=await run(bin,out,{gates:["creao"],triggerFetchImpl:completeTrigger});
  assert.equal(hosted.results.creao.status,"passed");
  assert.equal(hosted.results.creao.evidence.api_version,"trigger");
  const invalidOut=mkdtempSync(join(tmpdir(),"out-"));
  const invalid=await run(bin,invalidOut,{gates:["creao"],env:{...fullEnv,CREAO_TRIGGER_RUNS_URL:`https://agent.creao.ai/api/v1/apps/${RUN}/runs`}});
  assert.equal(invalid.results.creao.stage,"configuration");
  assert.equal(invalid.results.creao.status,"failed");
  assert.equal(readdirSync(invalidOut).includes("creao"),false);
});

test("blank API versions use Trigger and explicit legacy versions are refused", async () => {
  const bin=mkdtempSync(join(tmpdir(),"bin-"));
  for(const version of ["","  ","v1","v2","v3"]){
    const out=mkdtempSync(join(tmpdir(),"out-"));
    let requests=0;
    const r=await run(bin,out,{gates:["creao"],env:{...fullEnv,CREAO_API_VERSION:version},triggerFetchImpl:async(...args)=>{requests++;return completeTrigger(...args)}});
    if(version.trim()===""){
      assert.equal(r.results.creao.status,"passed");
      assert.equal(r.results.creao.evidence.api_version,"trigger");
      assert.equal(requests,2);
    }else{
      assert.equal(r.results.creao.status,"failed");
      assert.equal(r.results.creao.stage,"configuration");
      assert.match(r.results.creao.reason,/CREAO_API_VERSION must be trigger/);
      assert.equal(requests,0);
      assert.equal(readdirSync(out).includes("creao"),false);
    }
  }
});

test("apify stops at an unknown outcome and never steps again", async () => {
  const bin = mkdtempSync(join(tmpdir(), "bin-"));
  stub(
    bin,
    "genesis-forge",
    `if (args[0] === "prepare") process.exit(0);
fs.writeFileSync(stateFile, String(n + 1));
const seq = [{ phase: "actor_created", pending_action: "start_build" },
  { phase: "build_submitted", pending_action: "poll_build" },
  { phase: "needs_reconciliation", failure: { ambiguous: true, actor_id: "act-1", build_id: "b-1" } }];
const s = seq[Math.min(n, seq.length - 1)]; console.log(JSON.stringify(s));
process.exit(s.phase === "needs_reconciliation" ? 1 : 0);`,
  );
  const r = await run(bin, mkdtempSync(join(tmpdir(), "out-")), { gates: ["apify"] });
  assert.equal(r.results.apify.status, "needs_reconciliation");
  assert.equal(r.results.apify.evidence.actor_id, "act-1");
  assert.equal(calls(bin, "genesis-forge").filter((c) => c.startsWith("step")).length, 3, "no step after the unknown outcome");
  assert.match(r.results.apify.note, /unreachable/);
});

test("apify without a terminal phase is partial; a rejection is failed", async () => {
  const bin = mkdtempSync(join(tmpdir(), "bin-"));
  stub(
    bin,
    "genesis-forge",
    `if (args[0] === "prepare") process.exit(0);
fs.writeFileSync(stateFile, String(n + 1));
const art = { actor_id: "act-2", build_id: "b-2", build_number: "0.0.1" };
console.log(JSON.stringify(n === 0 ? { phase: "actor_created" } : { phase: "testing", artifact: art, completed_cases: 1, total_cases: 3, pending_action: "collect_case" }));`,
  );
  const r = await run(bin, mkdtempSync(join(tmpdir(), "out-")), { gates: ["apify"], maxSteps: 3 });
  assert.equal(r.results.apify.status, "partial");
  assert.equal(r.results.apify.evidence.build_id, "b-2");
  assert.match(r.results.apify.stopped, /step limit/);
  assert.match(r.results.apify.evidence.resume, /genesis-forge step --suite .* --checkpoint .*apify\/t1\/forge\.json/);
});

test("resume rendering survives spaces, quotes and shell metacharacters", async () => {
  const { shellQuote } = await import("./run.mjs");
  const { spawnSync } = await import("node:child_process");
  for (const arg of ["plain/path.json", "with space", "it's here", "$(touch x); `y` | z & *", ""]) {
    const r = spawnSync("/bin/sh", ["-c", `printf %s ${shellQuote(arg)}`], { encoding: "utf8" });
    assert.equal(r.stdout, arg);
  }
});
