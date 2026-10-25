import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { runTrigger } from "./creao-trigger.mjs";

const APP = "00000000-0000-4000-8000-000000000001";
const RUN = "11111111-1111-4111-8111-111111111111";
const KEY = "capi_offline-fixture-only";
const inputs = { task: "check a synthetic venue" };
const response = (body, status = 200) => new Response(JSON.stringify(body), {
  status, headers: { "Content-Type": "application/json" },
});

function fixture(t) {
  const dir = mkdtempSync(join(tmpdir(), "genesis-creao-trigger-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  return join(dir, "run.json");
}

test("preflight checks configuration without a provider call or journal", async (t) => {
  const out = fixture(t);
  const result = await runTrigger({ appId: APP, inputs, out, key: KEY,
    fetchImpl: () => { throw new Error("network forbidden"); } });
  assert.equal(result.phase, "preflight");
  assert.equal(result.key_present, true);
  assert.throws(() => readFileSync(out), /ENOENT/);
});

test("one app-scoped POST then exact-run GET; journal omits key and output", async (t) => {
  const out = fixture(t);
  const calls = [];
  const fetchImpl = async (url, options) => {
    calls.push({ url, options });
    if (options.method === "POST") return response({ id: RUN, appId: APP, status: "pending" }, 202);
    return response({ id: RUN, status: "completed",
      result: { text: "private generated answer" }, usage: { credits: 2 } });
  };
  const result = await runTrigger({ appId: APP, inputs, out, key: KEY,
    allowCredits: true, fetchImpl, pollMs: 0 });
  assert.equal(result.phase, "completed");
  assert.equal(result.credits_used, 2);
  assert.match(result.result_sha256, /^[0-9a-f]{64}$/);
  assert.equal(calls.length, 2);
  assert.equal(calls[0].url, `https://agent.creao.ai/api/v1/apps/${APP}/runs`);
  assert.equal(calls[0].options.headers.Authorization, `Bearer ${KEY}`);
  assert.deepEqual(JSON.parse(calls[0].options.body), { inputs });
  assert.equal(calls[1].url, `https://agent.creao.ai/api/v1/apps/${APP}/runs/${RUN}`);
  assert.match(calls[1].options.headers["User-Agent"], /Mozilla\/5\.0/);
  const saved = readFileSync(out, "utf8");
  assert.equal(saved.includes(KEY), false);
  assert.equal(saved.includes("private generated answer"), false);
  assert.equal(JSON.parse(saved).run_id, RUN);
  const again = await runTrigger({ appId: APP, inputs, out, key: KEY,
    allowCredits: true, fetchImpl });
  assert.equal(again.phase, "completed");
  assert.equal(calls.length, 2, "completed journal cannot submit again");
});

test("unknown POST stays blocked even when invoked again", async (t) => {
  const out = fixture(t);
  let calls = 0;
  const fetchImpl = async () => { calls++; throw new Error(`provider echoed ${KEY}`); };
  const first = await runTrigger({ appId: APP, inputs, out, key: KEY,
    allowCredits: true, fetchImpl });
  const second = await runTrigger({ appId: APP, inputs, out, key: KEY,
    allowCredits: true, fetchImpl });
  assert.equal(first.phase, "needs_reconciliation");
  assert.equal(second.phase, "needs_reconciliation");
  assert.equal(calls, 1);
  assert.equal(readFileSync(out, "utf8").includes(KEY), false);
  assert.equal(JSON.stringify(first).includes(KEY), false);
});

test("failed GET resumes only that GET and cannot switch app or input", async (t) => {
  const out = fixture(t);
  const methods = [];
  let reads = 0;
  const fetchImpl = async (_url, options) => {
    methods.push(options.method);
    if (options.method === "POST") return response({ id: RUN, appId: APP, status: "pending" }, 202);
    reads++;
    if (reads === 1) throw new Error("temporary read outage");
    return response({ id: RUN, appId: APP, status: "completed", result: { text: "done" } });
  };
  const first = await runTrigger({ appId: APP, inputs, out, key: KEY,
    allowCredits: true, fetchImpl });
  assert.equal(first.phase, "running");
  assert.match(first.reason, /poll_failed/);
  await assert.rejects(runTrigger({ appId: APP, inputs: { different: true }, out, key: KEY,
    allowCredits: true, fetchImpl }), /does not match/);
  const second = await runTrigger({ appId: APP, inputs, out, key: KEY,
    allowCredits: true, fetchImpl });
  assert.equal(second.phase, "completed");
  assert.deepEqual(methods, ["POST", "GET", "GET"]);
});

test("rejects wrong key, redirect and mismatched provider identity", async (t) => {
  const out1 = fixture(t);
  await assert.rejects(runTrigger({ appId: APP, inputs, out: out1, key: "cr_sk_wrong",
    allowCredits: true, fetchImpl: () => { throw new Error("network forbidden"); } }), /capi_/);
  const redirect = await runTrigger({ appId: APP, inputs, out: out1, key: KEY,
    allowCredits: true, fetchImpl: async () => new Response(null, { status: 302 }) });
  assert.equal(redirect.phase, "needs_reconciliation");
  const out2 = fixture(t);
  const mismatch = await runTrigger({ appId: APP, inputs, out: out2, key: KEY,
    allowCredits: true, fetchImpl: async () => response({ id: RUN,
      appId: "00000000-0000-4000-8000-000000000002", status: "pending" }, 202) });
  assert.equal(mismatch.phase, "needs_reconciliation");
  const out3 = fixture(t);
  const mismatchedGet = await runTrigger({ appId: APP, inputs, out: out3, key: KEY,
    allowCredits: true, fetchImpl: async (_url, options) => options.method === "POST"
      ? response({ id: RUN, appId: APP, status: "pending" }, 202)
      : response({ id: RUN, appId: "00000000-0000-4000-8000-000000000002",
        status: "completed" }) });
  assert.match(mismatchedGet.reason, /poll_failed/);
});
