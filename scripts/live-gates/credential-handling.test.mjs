// Offline regressions: synthetic credentials and stub CLIs only.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { runCli, runGates, scrub } from "./run.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const baseEnv = { PATH: process.env.PATH };

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "genesis-credential-review-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const binDir = join(root, "bin");
  mkdirSync(binDir);
  return { root, binDir, out: join(root, "out") };
}

function stub(binDir, name, source) {
  const path = join(binDir, name);
  writeFileSync(path, `#!/usr/bin/env node\n${source}\n`);
  chmodSync(path, 0o755);
}

test("direct CLI runs and resolves fixtures from a path with URL-escaped characters", (t) => {
  const f = fixture(t);
  const copiedRepo = join(f.root, "repo with spaces # percent% ü");
  for (const relative of [
    "scripts/live-gates/run.mjs",
    "actors/venue-extractor-template/tests/baseline-extractor.ts",
    "crates/genesis-forge/examples/synthesis/candidate.json",
  ]) {
    const target = join(copiedRepo, relative);
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(join(repo, relative), target);
  }
  stub(f.binDir, "genesis-forge", `
const fs = require("node:fs");
const args = process.argv.slice(2);
if (args[0] === "prepare") {
  const parser = args[args.indexOf("--extractor") + 1];
  if (!fs.readFileSync(parser, "utf8").includes("extract")) process.exit(2);
  process.exit(0);
}
console.log(JSON.stringify({ phase: "verified", artifact: { actor_id: "offline-actor", build_id: "offline-build" } }));`);
  const result = spawnSync(process.execPath, [join(copiedRepo, "scripts/live-gates/run.mjs"),
    "--gates", "apify", "--allow-billable", "--max-steps", "1", "--bin-dir", f.binDir, "--out", f.out],
  { cwd: f.root, env: { ...baseEnv, APIFY_TOKEN: "offline-path-credential" }, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /\| apify \| \*\*passed\*\*/);
  const reports = readdirSync(f.out).filter((name) => /^gates-.*\.json$/.test(name));
  assert.equal(reports.length, 1);
  assert.equal(JSON.parse(readFileSync(join(f.out, reports[0]), "utf8")).results.apify.status, "passed");
});

test("importing the runner does not execute its CLI", (t) => {
  const f = fixture(t);
  const result = spawnSync(process.execPath, ["--input-type=module", "--eval",
    `const runner = await import(${JSON.stringify(pathToFileURL(join(repo, "scripts/live-gates/run.mjs")).href)}); console.log(typeof runner.runGates);`],
  { cwd: f.root, env: baseEnv, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), "function");
  assert.equal(existsSync(join(f.root, "data")), false);
});

test("scrub covers supplied credential names and conventional credential suffixes", () => {
  const names = ["ELASTIC_DATA_API_KEY", "ELASTIC_MANAGEMENT_API_KEY", "AGENTVERSE_API_KEY",
    "APIFY_TOKEN", "CREAO_ACCOUNT_API_KEY", "CREAO_V2_API_KEY", "ASI_ONE_API_KEY",
    "GENESIS_LLM_API_KEY", "GENESIS_AGENT_TOKEN", "OTHER_SERVICE_TOKEN", "APP_SECRET",
    "SERVICE_PASSWORD", "GENESIS_ACP_SEED"];
  const env = Object.fromEntries(names.map((name, i) => [name, `offline-${i}-credential-end`]));
  const input = names.map((name) => ({ field: name, nested: { credential: env[name] } }));
  const output = scrub(input, env);
  assert.deepEqual(output.map((item) => item.nested.credential), names.map((name) => `<${name}>`));
  assert.deepEqual(output.map((item) => item.field), names);
});

test("scrub handles quoted and control-character values without corrupting JSON", () => {
  const secret = "offline-quote\"-slash\\-newline\n-tab\t-end";
  const env = { GENESIS_LLM_API_KEY: secret };
  const input = { [secret]: [secret, { raw: `before ${secret} after`, serialized: JSON.stringify({ key: secret }),
    encoded: encodeURIComponent(secret) }], unchanged: [false, true, null, 42] };
  const result = scrub(input, env);
  assert.deepEqual(result, {
    "<GENESIS_LLM_API_KEY>": ["<GENESIS_LLM_API_KEY>", {
      raw: "before <GENESIS_LLM_API_KEY> after",
      serialized: '{"key":"<GENESIS_LLM_API_KEY>"}', encoded: "<GENESIS_LLM_API_KEY>",
    }], unchanged: [false, true, null, 42],
  });
  assert.doesNotThrow(() => JSON.parse(JSON.stringify(result)));
  assert.equal(input[secret][0], secret, "scrubbing must not mutate private caller state");
});

test("scrub handles short credentials without changing JSON scalar values", () => {
  assert.deepEqual(scrub({ flag: false, count: 123, list: ["false", "123", ""] },
    { GENESIS_AGENT_TOKEN: "false", APP_SECRET: "123", EMPTY_TOKEN: "" }),
  { flag: false, count: 123, list: ["<GENESIS_AGENT_TOKEN>", "<APP_SECRET>", ""] });
});

test("overlapping secrets are replaced longest first in one pass", () => {
  const env = { ELASTIC_DATA_API_KEY: "offline-common-credential", ELASTIC_MANAGEMENT_API_KEY: "offline-common-credential-longer" };
  assert.equal(scrub(`${env.ELASTIC_MANAGEMENT_API_KEY}|${env.ELASTIC_DATA_API_KEY}`, env),
    "<ELASTIC_MANAGEMENT_API_KEY>|<ELASTIC_DATA_API_KEY>");
  assert.equal(scrub("credential-token", { APP_SECRET: "TOKEN", OTHER_TOKEN: "credential-token" }), "<OTHER_TOKEN>",
    "replacement markers must not be scrubbed again");
});

test("CLI error tails are scrubbed before truncation can expose a credential fragment", () => {
  const secret = `offline-long-${"x".repeat(650)}-tail`;
  const result = runCli(process.execPath, ["-e", "process.stderr.write('prefix ' + process.env.GENESIS_LLM_API_KEY + ' suffix'); process.exitCode = 1;"],
    { env: { ...baseEnv, GENESIS_LLM_API_KEY: secret } });
  assert.equal(result.code, 1);
  assert.equal(result.stderr, "prefix <GENESIS_LLM_API_KEY> suffix");
});

test("runner persists only scrubbed selected evidence and Markdown summaries", async (t) => {
  const f = fixture(t);
  const env = { ...baseEnv, AGENTVERSE_API_KEY: "offline-fetch-credential", CREAO_V2_API_KEY: "offline-v2-credential",
    ASI_ONE_API_KEY: "offline-asi-credential", GENESIS_LLM_API_KEY: "offline-model-credential", GENESIS_AGENT_TOKEN: "offline-agent-credential" };
  stub(f.binDir, "genesis-fetch", `console.log(JSON.stringify({ search_id: process.env.CREAO_V2_API_KEY,
    num_hits: 3, total: 3, candidates: [process.env.ASI_ONE_API_KEY, process.env.GENESIS_LLM_API_KEY, process.env.GENESIS_AGENT_TOKEN].map(address => ({ address })) }));`);
  const report = await runGates({ repo, ...f, gates: ["fetch"], env, stamp: "redaction" });
  assert.equal(report.results.fetch.status, "partial");
  assert.equal(report.results.fetch.evidence.search_id, "<CREAO_V2_API_KEY>");
  const outputs = [JSON.stringify(report), ...readdirSync(f.out).map((file) => readFileSync(join(f.out, file), "utf8"))];
  for (const [name, value] of Object.entries(env).filter(([name]) => name !== "PATH")) {
    assert.ok(outputs.every((output) => !output.includes(value)), `${name} must not appear in emitted evidence`);
  }
});

test("the retained Elastic smoke report is scrubbed, including non-selected details", async (t) => {
  const f = fixture(t);
  const env = { ...baseEnv, ELASTICSEARCH_URL: "https://offline.example", ELASTIC_KIBANA_URL: "https://offline.example",
    ELASTIC_DATA_API_KEY: "offline-data-credential", ELASTIC_MANAGEMENT_API_KEY: "offline-management-credential", GENESIS_LLM_API_KEY: "offline-model-credential" };
  stub(f.binDir, "genesis-elastic-smoke", `
const fs = require("node:fs"); const args = process.argv.slice(2);
fs.writeFileSync(args[args.indexOf("--report") + 1], JSON.stringify({ ok: true, steps: [
  { step: "non-selected diagnostic", ok: true, detail: process.env.GENESIS_LLM_API_KEY }
] }));`);
  const report = await runGates({ repo, ...f, gates: ["elastic"], env, stamp: "elastic" });
  assert.equal(report.results.elastic.status, "passed");
  const saved = JSON.parse(readFileSync(join(f.out, "elastic-smoke.json"), "utf8"));
  assert.equal(saved.steps[0].detail, "<GENESIS_LLM_API_KEY>");
  assert.equal(saved.steps[0].ok, true);
});

test("a malformed Elastic report does not leave an echoed credential on disk", async (t) => {
  const f = fixture(t);
  const env = { ...baseEnv, ELASTICSEARCH_URL: "https://offline.example", ELASTIC_KIBANA_URL: "https://offline.example",
    ELASTIC_DATA_API_KEY: "offline-data-credential", ELASTIC_MANAGEMENT_API_KEY: "offline-management-credential" };
  stub(f.binDir, "genesis-elastic-smoke", `const fs = require("node:fs"); const args = process.argv.slice(2);
fs.writeFileSync(args[args.indexOf("--report") + 1], '{"echo":"' + process.env.ELASTIC_DATA_API_KEY);`);
  const report = await runGates({ repo, ...f, gates: ["elastic"], env, stamp: "invalid" });
  assert.equal(report.results.elastic.status, "failed");
  assert.equal(readFileSync(join(f.out, "elastic-smoke.json"), "utf8").includes(env.ELASTIC_DATA_API_KEY), false);
});
