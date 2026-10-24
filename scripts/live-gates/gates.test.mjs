// node --test scripts/live-gates/gates.test.mjs
// Orchestration tests with stub CLIs: no network, no credentials.
import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { runGates, scrub } from "./run.mjs";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
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
  CREAO_ACCOUNT_API_KEY: SECRET + "c",
};
const run = (bin, out, extra = {}) =>
  runGates({ repo: REPO, binDir: bin, out, gates: ["elastic", "fetch", "apify", "creao"], env: fullEnv, stamp: "t1", pollScale: 0, allowBillable: true, ...extra });

test("missing credentials or binaries are not_tested, never a pass", async () => {
  const bin = mkdtempSync(join(tmpdir(), "bin-"));
  const out = mkdtempSync(join(tmpdir(), "out-"));
  const r = await runGates({ repo: REPO, binDir: bin, out, gates: ["elastic", "fetch", "apify", "creao"], env: { PATH: process.env.PATH }, stamp: "t0" });
  for (const g of Object.values(r.results)) {
    assert.equal(g.status, "not_tested", g.gate);
    assert.match(g.reason, /missing/);
  }
  const noBins = await run(bin, out);
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

test("creao steps until native completion and records the agent", async () => {
  const bin = mkdtempSync(join(tmpdir(), "bin-"));
  stub(
    bin,
    "genesis-creao",
    `if (args[0] === "prepare") { console.log(JSON.stringify({ phase: "prepared" })); process.exit(0); }
fs.writeFileSync(stateFile, String(n + 1));
const seq = [{ phase: "waiting", next_poll_at: new Date().toISOString() },
  { phase: "waiting", retryable_read: true }, { phase: "waiting", run_id: "run-1" },
  { phase: "native_completed", native_status: "completed", run_id: "run-1", provider_agent_id: "agent-9" }];
console.log(JSON.stringify(seq[Math.min(n, seq.length - 1)]));`,
  );
  const r = await run(bin, mkdtempSync(join(tmpdir(), "out-")), { gates: ["creao"] });
  assert.equal(r.results.creao.status, "passed");
  assert.equal(r.results.creao.evidence.provider_agent_id, "agent-9");
  assert.equal(r.results.creao.evidence.api_version, "v1");
  assert.equal(r.results.creao.evidence.readiness_verified, false);
  const steps = calls(bin, "genesis-creao").filter((c) => c.startsWith("step"));
  assert.equal(steps.length, 4);
  assert.ok(steps[2].includes("--retry-read"), "a retryable read is retried explicitly");
  assert.ok(!steps[1].includes("--retry-read") && !steps[3].includes("--retry-read"));
});

test("creao v2 gate freezes protocol in the prepared spec", async () => {
  const bin = mkdtempSync(join(tmpdir(), "bin-"));
  const out = mkdtempSync(join(tmpdir(), "out-"));
  stub(bin, "genesis-creao", `process.exit(1)`);
  const r = await run(bin, out, {
    gates: ["creao"],
    env: { ...fullEnv, CREAO_API_VERSION: "v2" },
  });
  assert.equal(r.results.creao.stage, "prepare");
  const spec = JSON.parse(readFileSync(join(out, "creao", "t1", "spec.json"), "utf8"));
  assert.equal(spec.api_version, "v2");
});

test("creao v2 gate labels public evidence and rejects invalid protocol before preparation", async () => {
  const bin = mkdtempSync(join(tmpdir(), "bin-"));
  const out = mkdtempSync(join(tmpdir(), "out-"));
  stub(bin, "genesis-creao", `if (args[0] === "prepare") process.exit(0);
console.log(JSON.stringify({ phase: "native_completed", native_status: "completed", run_id: "run-2", provider_agent_id: "agent-2" }));`);
  const hosted = await run(bin, out, {
    gates: ["creao"],
    env: { ...fullEnv, CREAO_API_VERSION: "v2" },
  });
  assert.equal(hosted.results.creao.status, "passed");
  assert.equal(hosted.results.creao.evidence.api_version, "v2");
  const invalidOut = mkdtempSync(join(tmpdir(), "out-"));
  const priorCalls = calls(bin, "genesis-creao").length;
  const invalid = await run(bin, invalidOut, {
    gates: ["creao"],
    env: { ...fullEnv, CREAO_API_VERSION: "v3" },
  });
  assert.equal(invalid.results.creao.stage, "configuration");
  assert.equal(invalid.results.creao.status, "failed");
  assert.equal(calls(bin, "genesis-creao").length, priorCalls);
  assert.equal(readdirSync(invalidOut).includes("creao"), false);
});

test("blank creao protocol uses v1 consistently with the backend", async () => {
  const bin = mkdtempSync(join(tmpdir(), "bin-"));
  stub(bin, "genesis-creao", `if (args[0] === "prepare") process.exit(0);
console.log(JSON.stringify({ phase: "native_completed", native_status: "completed", run_id: "run-blank", provider_agent_id: "agent-blank" }));`);
  for (const version of ["", "  "]) {
    const out = mkdtempSync(join(tmpdir(), "out-"));
    const result = await run(bin, out, {
      gates: ["creao"],
      env: { ...fullEnv, CREAO_API_VERSION: version },
    });
    assert.equal(result.results.creao.status, "passed");
    assert.equal(result.results.creao.evidence.api_version, "v1");
    const spec = JSON.parse(readFileSync(join(out, "creao", "t1", "spec.json"), "utf8"));
    assert.equal(spec.api_version ?? "v1", "v1");
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
