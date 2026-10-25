// Independent retained-data guards. No runner, family, sandbox, engine, or provider
// module is imported. The verifier entrypoint runs in a VM with inert dependencies.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";

const file = fileURLToPath(new URL("./verify.mjs", import.meta.url));
const root = path.dirname(file);
const repo = path.resolve(root, "../..");
const source = fs.readFileSync(file, "utf8").replace(/^import .*;\n/gm, "").replaceAll("import.meta.url", JSON.stringify(new URL("./verify.mjs", import.meta.url).href));
assert.doesNotMatch(source, /\bimport\s/);
const sha256 = (value) => createHash("sha256").update(String(value)).digest("hex");
function canon(value) {
  if (typeof value === "number") return Object.is(value, -0) ? "#-0" : Number.isFinite(value) ? JSON.stringify(value) : "#" + String(value);
  if (value === undefined) return "null";
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canon).join(",")}]`;
  return `{${Object.keys(value).filter((key) => value[key] !== undefined).sort().map((key) => `${JSON.stringify(key)}:${canon(value[key])}`).join(",")}}`;
}
const SEEDS = Array.from({ length: 5 }, (_, i) => `judge-20261025-${String(i + 1).padStart(2, "0")}`);
const ARMS = ["independent", "feedback"];
const HEAD = "a".repeat(40);
const SOURCE_FILES = ["experiments/jobshop-controlled/PROTOCOL.md", "experiments/jobshop-controlled/verify.mjs"];
const frozenPanels = (seed) => ({ selection: Array.from({ length: 5 }, (_, i) => sha256(`${seed}/${i}`)), final: sha256(`${seed}/final`) });
const charge = (usage) => {
  assert.ok(usage && Number.isSafeInteger(usage.input_tokens) && Number.isSafeInteger(usage.output_tokens));
  return usage.input_tokens * 2 + usage.output_tokens * 10;
};
function scoreRow(label, successes = 30) {
  const source = `inert retained source ${label}`; // never evaluated
  const scores = Array.from({ length: 60 }, (_, i) => ({ success: i < successes, detail: { ratio: i < successes ? 1.1 : 1.4 } }));
  return { package: sha256(source), source, success_count: successes, of: 60, invalid_count: 0,
    mean_ratio: scores.reduce((sum, score) => sum + score.detail.ratio, 0) / 60, scores };
}
function fixture() {
  const manifest = { schema: "jobshop-controlled/1", source_head: HEAD,
    source_files: Object.fromEntries(SOURCE_FILES.map((name) => [name, sha256(name)])),
    protocol_sha256: sha256(SOURCE_FILES[0]), model: "claude-sonnet-5-5", effort: "medium", max_output_tokens: 8000,
    fallbacks: "disabled", calls: 50, generations: 5, candidates_kept: 4, candidates_screened: 8, selection_instances: 20, final_instances: 60,
    seeds: SEEDS, panels: Object.fromEntries(SEEDS.map((seed) => [seed, frozenPanels(seed)])),
    cost: { run_cap_micro_usd: 20_000_000, study_cap_micro_usd: 50_000_000, prior_project_reserve_micro_usd: 20_000_000,
      per_call_reserve_micro_usd: 204096, full_reserve_micro_usd: 10204800 } };
  const files = { "manifest.json": manifest };
  const events = [{ event: "run_started", full_reserve_micro_usd: 10204800 }];
  const finals = {}, winners = {};
  let requestIndex = 0;
  for (const [repetition, seed] of SEEDS.entries()) {
    finals[seed] = { controls: Object.fromEntries(["fixed-spt", "fixed-mwkr", "fixed-lpt", "random-search"].map((id) => [id, scoreRow(`${seed}/${id}`)])) };
    winners[seed] = {};
    for (const arm of repetition % 2 === 0 ? ["feedback", "independent"] : ARMS) {
      const final = scoreRow(`${seed}/${arm}`);
      finals[seed][arm] = final;
      winners[seed][arm] = final.package;
      const rows = [];
      for (let round = 1; round <= 5; round++) {
        const index = requestIndex++, identity = { seed, arm, round };
        files[`request-${String(index).padStart(3, "0")}.json`] = { model: manifest.model, max_tokens: 8000, output_config: { effort: "medium" }, identity: { ...identity } };
        const response = { model: manifest.model, stop_reason: "end_turn", usage: { input_tokens: 100, output_tokens: 10 } };
        files[`response-${String(index).padStart(3, "0")}.json`] = response;
        events.push({ event: "provider_pending", index, identity: { ...identity }, reserved_micro_usd: 204096 });
        events.push({ event: "provider_settled", index, identity: { ...identity }, model: manifest.model, stop_reason: response.stop_reason,
          usage: { ...response.usage }, charge_micro_usd: charge(response.usage) });
        const row = { round, plan: sha256(`${seed}/${arm}/${round}/plan`), promoted: null, candidates: [], engine_ledger_head: sha256(`${seed}/${arm}/${round}/ledger`),
          engine_evaluations: round * 100, screening_evaluations: round * 24, provider_calls: index + 1, spent_micro_usd: (index + 1) * 300 };
        rows.push(row);
        events.push({ event: "round_completed", seed, arm, ...row });
      }
      files[`state-${seed}-${arm}.json`] = { state: { lab: "judged_demo", seed, ledger: { head: rows.at(-1).engine_ledger_head },
        frontier: { "jobshop-10x5": { package: { sha256: final.package, source: final.source } } }, packages: {} }, rows };
    }
  }
  events.push({ event: "final_panel_opened", winners, calls: 50, settled_micro_usd: 15000 });
  for (const seed of SEEDS) events.push({ event: "final_repetition_completed", seed });
  files["final-scores.json"] = finals;
  files["summary.json"] = { status: "complete", manifest_sha256: sha256(canon(manifest)), source_head: HEAD,
    usage: { calls: 50, settled_micro_usd: 15000, input_tokens: 5000, output_tokens: 500 }, differences: [0, 0, 0, 0, 0],
    mean_difference: 0, interval_95: [0, 0], positive_repetitions: 0, declared_success: false, final_scores: finals };
  events.push({ event: "run_completed", spent_micro_usd: 15000 });
  return { files, events };
}
function verify(fixture) {
  const { files, events } = fixture;
  let previous = sha256("jobshop-controlled/1");
  const records = events.map((original, seq) => {
    const event = { ...original, seq, previous };
    if (event.event === "run_started") event.manifest_sha256 = sha256(canon(files["manifest.json"]));
    if (event.event === "provider_pending") event.request_sha256 = sha256(canon(files[`request-${String(event.index).padStart(3, "0")}.json`]));
    if (event.event === "provider_settled") event.response_sha256 = sha256(canon(files[`response-${String(event.index).padStart(3, "0")}.json`]));
    if (event.event === "final_repetition_completed") event.final_sha256 = sha256(canon(files["final-scores.json"][event.seed]));
    if (event.event === "run_completed" && files["summary.json"]) {
      files["summary.json"].request_journal = { head: previous, records: seq };
      event.summary_sha256 = sha256(canon(files["summary.json"]));
    }
    previous = sha256(canon(event));
    return JSON.stringify({ ...event, hash: previous });
  });
  const memory = new Map(Object.entries(files).map(([name, value]) => [path.join("/inert-evidence", name), JSON.stringify(value)]));
  memory.set("/inert-evidence/events.ndjson", records.join("\n"));
  for (const name of SOURCE_FILES) memory.set(path.join(repo, name), name);
  let output = "";
  vm.runInNewContext(source, {
    fs: { readFileSync: (name) => { assert.ok(memory.has(name), `unexpected file read: ${name}`); return memory.get(name); }, existsSync: (name) => memory.has(name) },
    path, createHash, fileURLToPath, canon, sha256, frozenPanels, usageCharge: charge, SOURCE_FILES,
    verifyLedger: (ledger) => ({ ok: true, head: ledger.head }), // ledger verification itself is outside these guards
    execFileSync: (command, args) => { assert.equal(command, "git"); assert.deepEqual(Array.from(args), ["rev-parse", "HEAD"]); return HEAD; },
    os: { release: () => { throw new Error("OS probe forbidden"); } },
    jobshopFamily: () => { throw new Error("family/evaluator execution forbidden"); },
    process: { argv: ["node", file, "--output-dir", "/inert-evidence", "--expected-head", previous], stdout: { write: (text) => { output += text; } } },
  }, { timeout: 1000, filename: file });
  return JSON.parse(output);
}

test("inert complete null result verifies without any evaluation", () => {
  assert.equal(verify(fixture()).status, "complete");
});
test("an unresolved submission stays incomplete without becoming a zero-cost completion", () => {
  const data = fixture();
  data.events = data.events.slice(0, 2);
  delete data.files["summary.json"];
  const result = verify(data);
  assert.equal(result.status, "incomplete");
  assert.equal(result.requests, 1);
  assert.equal(result.settled, 0);
});
const invalid = [
  ["false positive benefit claim", (f) => { Object.assign(f.files["summary.json"], { mean_difference: 0.2, interval_95: [0.1, 0.3], positive_repetitions: 5, declared_success: true }); }],
  ["wrong confidence interval", (f) => { f.files["summary.json"].interval_95 = [-0.1, 0.1]; }],
  ["non-complete summary status", (f) => { f.files["summary.json"].status = "verified-positive"; }],
  ["summary from another source", (f) => { f.files["summary.json"].source_head = "b".repeat(40); }],
  ["final score count not supported by score rows", (f) => { f.files["final-scores.json"][SEEDS[0]].controls["random-search"].success_count = 60; }],
  ["short final panel", (f) => { f.files["final-scores.json"][SEEDS[0]].controls["random-search"].scores.pop(); }],
  ["different bytes under a final package digest", (f) => { f.files["final-scores.json"][SEEDS[0]].feedback.source = "different inert source"; }],
  ["missing fixed control", (f) => { delete f.files["final-scores.json"][SEEDS[0]].controls["fixed-lpt"]; }],
  ["a request round differs from its journal identity", (f) => { f.files["request-000.json"].identity.round = 5; }],
  ["settlement belongs to a different round", (f) => { f.events.find((e) => e.event === "provider_settled").identity.round = 5; }],
  ["repeated request identity despite 50 distinct indices", (f) => { f.files["request-001.json"].identity.round = 1; f.events.find((e) => e.event === "provider_pending" && e.index === 1).identity.round = 1; f.events.find((e) => e.event === "provider_settled" && e.index === 1).identity.round = 1; }],
  ["selected package changed after final opening", (f) => { f.events.find((e) => e.event === "final_panel_opened").winners[SEEDS[0]].feedback = "0".repeat(64); }],
  ["missing final repetition commitment", (f) => { f.events = f.events.filter((e) => !(e.event === "final_repetition_completed" && e.seed === SEEDS[0])); }],
  ["completion after a stopped run", (f) => { f.events.splice(1, 0, { event: "run_stopped", reason: "unknown charge" }); }],
  ["events appended after completion", (f) => { f.events.push({ event: "mutator_observation", identity: { seed: SEEDS[0], arm: "feedback", round: 1 }, record: {} }); }],
  ["round before its settled request", (f) => { const at = f.events.findIndex((e) => e.event === "round_completed"); [f.events[at], f.events[at - 1]] = [f.events[at - 1], f.events[at]]; }],
  ["duplicate persisted round replacing an expected round", (f) => { const rows = f.files[`state-${SEEDS[0]}-feedback.json`].rows; rows[1] = { ...rows[0] }; }],
];
for (const [name, mutate] of invalid) test(`retained verifier rejects ${name}`, () => {
  const data = fixture();
  mutate(data);
  assert.throws(() => verify(data));
});

function setBenefits(data, differences) {
  for (const [i, seed] of SEEDS.entries()) {
    const row = data.files["final-scores.json"][seed].feedback;
    Object.assign(row, scoreRow(`${seed}/feedback`, 30 + differences[i] * 60));
  }
  const mean = differences.reduce((sum, x) => sum + x, 0) / 5;
  const halfWidth = 2.776 * Math.sqrt(differences.reduce((sum, x) => sum + (x - mean) ** 2, 0) / 4 / 5);
  const positive = differences.filter((x) => x > 0).length;
  Object.assign(data.files["summary.json"], { differences, mean_difference: mean, interval_95: [mean - halfWidth, mean + halfWidth],
    positive_repetitions: positive, declared_success: mean >= 0.05 && mean - halfWidth > 0 && positive >= 4 });
}
test("a supported positive result and a negative result both verify", () => {
  for (const differences of [[0.1, 0.2, 0.1, 0.2, 0.15], [-0.1, -0.2, -0.1, -0.2, -0.15]]) {
    const data = fixture();
    setBenefits(data, differences);
    assert.equal(verify(data).status, "complete");
  }
});
test("retained scoring failure stays unsuccessful with unknown mean ratio", () => {
  const data = fixture();
  const row = data.files["final-scores.json"][SEEDS[0]].controls["random-search"];
  row.scores[59] = { success: false, detail: { error: "inert retained failure" } };
  row.invalid_count = 1;
  row.mean_ratio = null;
  assert.equal(verify(data).status, "complete");
});
test("provider stop with its subsequent error observation remains incomplete", () => {
  const data = fixture();
  data.events = data.events.slice(0, 2);
  data.events.push({ event: "run_stopped", reason: "unknown charge" },
    { event: "mutator_observation", identity: data.events[1].identity, record: { error: "inert submission failure" } });
  delete data.files["summary.json"];
  assert.equal(verify(data).status, "incomplete");
});
for (const [name, mutate] of [
  ["different frozen effort", (f) => { f.files["manifest.json"].effort = "high"; }],
  ["different final panel size", (f) => { f.files["manifest.json"].final_instances = 30; }],
  ["duplicate seed roster", (f) => { f.files["manifest.json"].seeds = [SEEDS[0], ...SEEDS.slice(0, 4)]; }],
  ["lower unposted-usage reserve", (f) => { f.files["manifest.json"].cost.prior_project_reserve_micro_usd = 0; }],
  ["unknown success represented as truthy text", (f) => { f.files["final-scores.json"][SEEDS[0]].controls["random-search"].scores[0].success = "unknown"; }],
  ["failed ratio falsely represented as a numeric mean", (f) => { const row = f.files["final-scores.json"][SEEDS[0]].controls["random-search"]; row.scores[59] = { success: false }; row.invalid_count = 1; }],
  ["persisted round metrics disagree with the journal", (f) => { f.files[`state-${SEEDS[0]}-feedback.json`].rows[0].engine_evaluations = 999; }],
  ["persisted round silently drops its plan", (f) => { delete f.files[`state-${SEEDS[0]}-feedback.json`].rows[0].plan; }],
]) test(`retained verifier also rejects ${name}`, () => {
  const data = fixture();
  mutate(data);
  assert.throws(() => verify(data));
});

function stoppedBeforeSubmission(completedRounds = 0) {
  const data = fixture();
  const next = data.events.find((event) => event.event === "provider_pending" && event.index === completedRounds).identity;
  data.events = data.events.slice(0, 1 + completedRounds * 3);
  data.events.push({ event: "run_stopped", reason: "provider or cost uncertainty",
    detail: "request does not meet frozen model or size settings", calls_submitted: completedRounds, settled_micro_usd: completedRounds * 300 },
  { event: "mutator_observation", identity: next, record: { generation: next.round, error: "request does not meet frozen model or size settings" } });
  delete data.files["summary.json"];
  return data;
}
test("pre-submission rejection preserves an incomplete run at its first or next position", () => {
  for (const completed of [0, 1]) {
    const result = verify(stoppedBeforeSubmission(completed));
    assert.equal(result.status, "incomplete");
    assert.equal(result.requests, completed);
    assert.equal(result.settled, completed);
  }
});
for (const [name, mutate] of [
  ["without a stop", (f) => { f.events.splice(1, 1); }],
  ["for another round", (f) => { f.events.at(-1).identity.round = 5; }],
  ["for another arm", (f) => { f.events.at(-1).identity.arm = "independent"; }],
  ["for another seed", (f) => { f.events.at(-1).identity.seed = SEEDS[1]; }],
  ["with a successful observation", (f) => { f.events.at(-1).record = { generation: 1, candidates: 8 }; }],
  ["with a different generation", (f) => { f.events.at(-1).record.generation = 2; }],
  ["followed by settlement", (f) => { f.events.push(fixture().events.find((event) => event.event === "provider_settled")); }],
  ["followed by completion", (f) => { f.events.push({ event: "run_completed", spent_micro_usd: 0 }); }],
]) test(`pre-submission error observation is rejected ${name}`, () => {
  const data = stoppedBeforeSubmission();
  mutate(data);
  assert.throws(() => verify(data));
});
