// Recheck retained local evidence. --recompute-final reruns every final score.
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { canon, sha256, verifyLedger } from "../../scripts/iteration/engine.mjs";
import { jobshopFamily } from "../../scripts/iteration/families/jobshop.mjs";
import { frozenPanels, usageCharge, SOURCE_FILES } from "./run.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(ROOT, "../..");
const arg = (name) => { const i = process.argv.indexOf(`--${name}`); return i < 0 ? null : process.argv[i + 1]; };
const output = arg("output-dir");
if (!output) throw new Error("--output-dir is required");
const dir = path.resolve(output);
const read = (name) => JSON.parse(fs.readFileSync(path.join(dir, name), "utf8"));
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const manifest = read("manifest.json");
const currentHead = execFileSync("git", ["rev-parse", "HEAD"], { cwd: ROOT, encoding: "utf8" }).trim();
assert(manifest.source_head === currentHead, "source head differs from retained run");
assert(manifest.protocol_sha256 === sha256(fs.readFileSync(path.join(ROOT, "PROTOCOL.md"), "utf8")), "protocol bytes differ");
const hashFile = (file) => createHash("sha256").update(fs.readFileSync(file)).digest("hex");
assert(canon(Object.keys(manifest.source_files).sort()) === canon([...SOURCE_FILES].sort()), "source file roster differs");
for (const name of SOURCE_FILES) assert(manifest.source_files[name] === hashFile(path.join(REPO, name)), `source bytes differ: ${name}`);
if (process.argv.includes("--recompute-final")) {
  assert(manifest.runtime.node === process.version && manifest.runtime.platform === process.platform && manifest.runtime.arch === process.arch && manifest.runtime.os_release === os.release() && manifest.runtime.executable_sha256 === hashFile(process.execPath), "final replay runtime differs");
}
assert(manifest.schema === "jobshop-controlled/1" && manifest.model === "claude-sonnet-5-5" && manifest.calls === 50, "unexpected manifest design");
for (const seed of manifest.seeds) assert(canon(manifest.panels[seed]) === canon(frozenPanels(seed)), `panel commitment differs: ${seed}`);

let previous = sha256("jobshop-controlled/1");
let index = 0;
const pending = new Map();
const settled = new Map();
const rounds = new Map();
let completed = null;
let spent = 0;
let input = 0;
let outputTokens = 0;
let finalOpened = false;
for (const line of fs.readFileSync(path.join(dir, "events.ndjson"), "utf8").trim().split("\n")) {
  const { hash, ...event } = JSON.parse(line);
  assert(event.seq === index++ && event.previous === previous && sha256(canon(event)) === hash, "event chain mismatch");
  previous = hash;
  if (event.event === "provider_pending") {
    assert(event.index === pending.size && !pending.has(event.index), "missing or duplicate request index");
    const request = read(`request-${String(event.index).padStart(3, "0")}.json`);
    assert(sha256(canon(request)) === event.request_sha256 && request.identity.seed === event.identity.seed && request.identity.arm === event.identity.arm, "request binding mismatch");
    assert(request.model === manifest.model && request.max_tokens === manifest.max_output_tokens && request.output_config.effort === manifest.effort, "request settings differ");
    pending.set(event.index, event);
  }
  if (event.event === "provider_settled") {
    assert(pending.has(event.index) && !settled.has(event.index), "settlement without one pending request");
    const response = read(`response-${String(event.index).padStart(3, "0")}.json`);
    assert(sha256(canon(response)) === event.response_sha256 && response.model === manifest.model, "response binding or model mismatch");
    const micro = usageCharge(response.usage);
    assert(event.charge_micro_usd === micro && canon(event.usage) === canon(response.usage), "usage or charge mismatch");
    spent += micro;
    input += response.usage.input_tokens;
    outputTokens += response.usage.output_tokens;
    settled.set(event.index, event);
  }
  if (event.event === "round_completed") {
    const key = `${event.seed}/${event.arm}/${event.round}`;
    assert(!rounds.has(key), "duplicate round");
    rounds.set(key, event);
  }
  if (event.event === "final_panel_opened") {
    assert(!finalOpened && settled.size === manifest.calls && rounds.size === manifest.calls, "final panel opened before full closure");
    finalOpened = true;
  }
  if (event.event === "run_completed") completed = event;
}
assert(!arg("expected-head") || arg("expected-head") === previous, "published event head differs");
const summary = fs.existsSync(path.join(dir, "summary.json")) ? read("summary.json") : null;
if (summary) {
  assert(completed && finalOpened && settled.size === manifest.calls && pending.size === manifest.calls, "completed run lacks 50 settled requests");
  assert(completed.summary_sha256 === sha256(canon(summary)), "summary hash mismatch");
  assert(summary.request_journal.head === completed.previous && summary.request_journal.records === completed.seq, "summary journal anchor mismatch");
  assert(summary.usage.calls === manifest.calls && summary.usage.settled_micro_usd === spent && summary.usage.input_tokens === input && summary.usage.output_tokens === outputTokens, "summary usage mismatch");
  assert(spent <= manifest.cost.run_cap_micro_usd && spent + manifest.cost.prior_project_reserve_micro_usd <= manifest.cost.study_cap_micro_usd, "charge exceeds a cap");
  const finals = read("final-scores.json");
  assert(canon(summary.final_scores) === canon(finals), "summary final scores differ from retained final scores");
  for (const seed of manifest.seeds) {
    for (const arm of ["independent", "feedback"]) {
      const { state, rows } = read(`state-${seed}-${arm}.json`);
      const checked = verifyLedger(state.ledger);
      assert(checked.ok && rows.length === 5, `engine ledger or round count failed: ${seed}/${arm}`);
      for (const row of rows) assert(rounds.get(`${seed}/${arm}/${row.round}`)?.engine_ledger_head === row.engine_ledger_head, "round binding mismatch");
      assert(rows.at(-1).engine_ledger_head === checked.head, "final engine head mismatch");
      const selected = state.frontier["jobshop-10x5"]?.package || state.packages[sha256("(op) => -op.duration")];
      assert(selected.sha256 === finals[seed][arm].package, "final selected package differs");
    }
    if (process.argv.includes("--recompute-final")) {
      const family = jobshopFamily({ id: "jobshop-10x5", seed });
      const instances = family.instances(sha256(`final:${seed}`), 60);
      for (const row of [finals[seed].independent, finals[seed].feedback, ...Object.values(finals[seed].controls)]) {
        const scores = instances.map((instance) => family.score({ sha256: row.package, source: row.source }, instance));
        assert(canon(scores) === canon(row.scores) && row.success_count === scores.filter((x) => x.success).length, "final score replay differs");
      }
    }
  }
  const differences = manifest.seeds.map((seed) => (finals[seed].feedback.success_count - finals[seed].independent.success_count) / 60);
  assert(canon(differences) === canon(summary.differences), "paired final differences differ");
}
process.stdout.write(JSON.stringify({ status: summary?.status || "incomplete", events: index, head: previous, requests: pending.size,
  settled: settled.size, spent_usd: spent / 1e6, recomputed_final: process.argv.includes("--recompute-final") }) + "\n");
