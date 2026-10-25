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
const same = (a, b) => canon(a) === canon(b);
const SEEDS = Array.from({ length: 5 }, (_, i) => `judge-20261025-${String(i + 1).padStart(2, "0")}`);
const ARMS = ["independent", "feedback"];
const CONTROLS = ["fixed-spt", "fixed-mwkr", "fixed-lpt", "random-search"];
const positions = SEEDS.flatMap((seed, i) => (i % 2 === 0 ? ["feedback", "independent"] : ARMS)
  .flatMap((arm) => Array.from({ length: 5 }, (_, round) => ({ seed, arm, round: round + 1 }))));
const identity = ({ seed, arm, round }) => ({ seed, arm, round });
const roundKey = ({ seed, arm, round }) => `${seed}/${arm}/${round}`;
const manifest = read("manifest.json");
const currentHead = execFileSync("git", ["rev-parse", "HEAD"], { cwd: ROOT, encoding: "utf8" }).trim();
assert(manifest.source_head === currentHead, "source head differs from retained run");
assert(manifest.protocol_sha256 === sha256(fs.readFileSync(path.join(ROOT, "PROTOCOL.md"), "utf8")), "protocol bytes differ");
const hashFile = (file) => createHash("sha256").update(fs.readFileSync(file)).digest("hex");
assert(canon(Object.keys(manifest.source_files).sort()) === canon([...SOURCE_FILES].sort()), "source file roster differs");
for (const name of SOURCE_FILES) assert(manifest.source_files[name] === hashFile(path.join(REPO, name)), `source bytes differ: ${name}`);
if (process.argv.includes("--recompute-final")) {
  assert(manifest.runtime.node === process.version && manifest.runtime.platform === process.platform && manifest.runtime.arch === process.arch && manifest.runtime.os_release === os.release() && manifest.runtime.executable_sha256 === hashFile(process.execPath), "final replay runtime differs");
  const probeFamily = jobshopFamily({ id: "jobshop-10x5", seed: "admission-probe" });
  const probeSource = probeFamily.root();
  const probe = probeFamily.score({ sha256: sha256(probeSource), source: probeSource }, "jobshop-10x5/admission-probe/0");
  assert(manifest.sandbox_probe.rule_sha256 === sha256(probeSource) && canon(manifest.sandbox_probe.score) === canon(probe), "sandbox preflight replay differs");
}
assert(manifest.schema === "jobshop-controlled/1" && manifest.model === "claude-sonnet-5-5" && manifest.calls === 50, "unexpected manifest design");
assert(same(manifest.seeds, SEEDS) && same(Object.keys(manifest.panels).sort(), [...SEEDS].sort()), "repetition roster differs");
assert(manifest.effort === "medium" && manifest.max_output_tokens === 8000 && manifest.fallbacks === "disabled"
  && manifest.generations === 5 && manifest.candidates_kept === 4 && manifest.candidates_screened === 8
  && manifest.selection_instances === 20 && manifest.final_instances === 60, "frozen experiment settings differ");
assert(manifest.cost.run_cap_micro_usd === 20_000_000 && manifest.cost.study_cap_micro_usd === 50_000_000
  && manifest.cost.per_call_reserve_micro_usd === 204096 && manifest.cost.full_reserve_micro_usd === 10204800
  && Number.isSafeInteger(manifest.cost.prior_project_reserve_micro_usd) && manifest.cost.prior_project_reserve_micro_usd >= 20_000_000
  && manifest.cost.prior_project_reserve_micro_usd + manifest.cost.full_reserve_micro_usd <= manifest.cost.study_cap_micro_usd, "frozen cost reservation differs");
for (const seed of manifest.seeds) assert(canon(manifest.panels[seed]) === canon(frozenPanels(seed)), `panel commitment differs: ${seed}`);

let previous = sha256("jobshop-controlled/1"), index = 0;
const pending = new Map();
const settled = new Map();
const rounds = new Map();
const finalRepetitions = new Map();
const eventTypes = new Set(["run_started", "provider_pending", "provider_settled", "mutator_observation", "round_completed", "final_panel_opened", "final_repetition_completed", "run_completed", "run_stopped"]);
let completed = null, spent = 0, input = 0, outputTokens = 0, finalOpened = null, started = false, stopped = false;
for (const line of fs.readFileSync(path.join(dir, "events.ndjson"), "utf8").trim().split("\n")) {
  const { hash, ...event } = JSON.parse(line);
  assert(event.seq === index++ && event.previous === previous && sha256(canon(event)) === hash, "event chain mismatch");
  previous = hash;
  assert(eventTypes.has(event.event) && !completed, "unknown event or event after completion");
  // A failed provider call can still append the mutator's error observation.
  assert(!stopped || event.event === "mutator_observation", "work continued after run stopped");
  if (event.event === "run_started") {
    assert(!started && event.seq === 0 && event.manifest_sha256 === sha256(canon(manifest))
      && event.full_reserve_micro_usd === manifest.cost.full_reserve_micro_usd, "run start does not bind the manifest");
    started = true;
  }
  assert(started, "journal does not begin with a run start");
  if (event.event === "provider_pending") {
    assert(!finalOpened && event.index === pending.size && event.index < positions.length
      && settled.size === event.index && rounds.size === event.index, "missing, duplicate or out-of-order request index");
    assert(same(event.identity, positions[event.index]) && event.reserved_micro_usd === manifest.cost.per_call_reserve_micro_usd, "request position or reservation differs");
    const request = read(`request-${String(event.index).padStart(3, "0")}.json`);
    assert(sha256(canon(request)) === event.request_sha256 && same(request.identity, event.identity), "request binding mismatch");
    assert(request.model === manifest.model && request.max_tokens === manifest.max_output_tokens && request.output_config.effort === manifest.effort, "request settings differ");
    pending.set(event.index, event);
  }
  if (event.event === "provider_settled") {
    assert(!finalOpened && pending.has(event.index) && event.index === settled.size, "settlement without one pending request");
    assert(same(event.identity, pending.get(event.index).identity), "settlement identity differs from pending request");
    const response = read(`response-${String(event.index).padStart(3, "0")}.json`);
    assert(sha256(canon(response)) === event.response_sha256 && response.model === manifest.model, "response binding or model mismatch");
    assert(event.model === response.model && event.stop_reason === response.stop_reason, "settlement outcome differs from response");
    const micro = usageCharge(response.usage);
    assert(event.charge_micro_usd === micro && canon(event.usage) === canon(response.usage), "usage or charge mismatch");
    spent += micro;
    input += response.usage.input_tokens;
    outputTokens += response.usage.output_tokens;
    settled.set(event.index, event);
  }
  if (event.event === "mutator_observation") assert(pending.size > 0 && !finalOpened
    && same(event.identity, pending.get(pending.size - 1).identity), "mutator observation identity differs");
  if (event.event === "round_completed") {
    const key = roundKey(event);
    assert(!finalOpened && !rounds.has(key) && same(identity(event), positions[rounds.size])
      && settled.size === rounds.size + 1 && pending.size === settled.size, "round has no matching settled position");
    assert(event.provider_calls === pending.size && event.spent_micro_usd === spent, "round usage differs from settled journal");
    rounds.set(key, event);
  }
  if (event.event === "final_panel_opened") {
    assert(!finalOpened && settled.size === manifest.calls && rounds.size === manifest.calls, "final panel opened before full closure");
    assert(event.calls === pending.size && event.settled_micro_usd === spent, "final opening usage differs");
    assert(same(Object.keys(event.winners).sort(), [...SEEDS].sort())
      && SEEDS.every((seed) => same(Object.keys(event.winners[seed]).sort(), [...ARMS].sort())), "frozen winner roster differs");
    finalOpened = event;
  }
  if (event.event === "final_repetition_completed") {
    assert(finalOpened && event.seed === SEEDS[finalRepetitions.size] && !finalRepetitions.has(event.seed), "final repetition missing, duplicate or out of order");
    finalRepetitions.set(event.seed, event);
  }
  if (event.event === "run_stopped") stopped = true;
  if (event.event === "run_completed") {
    assert(finalOpened && finalRepetitions.size === SEEDS.length && event.spent_micro_usd === spent, "completion lacks final repetition closure");
    completed = event;
  }
}
assert(!arg("expected-head") || arg("expected-head") === previous, "published event head differs");
const summary = fs.existsSync(path.join(dir, "summary.json")) ? read("summary.json") : null;
assert(!completed || summary, "completed journal lacks summary");
if (summary) {
  assert(completed && finalOpened && settled.size === manifest.calls && pending.size === manifest.calls, "completed run lacks 50 settled requests");
  assert(completed.summary_sha256 === sha256(canon(summary)), "summary hash mismatch");
  assert(summary.status === "complete" && summary.source_head === manifest.source_head
    && summary.manifest_sha256 === sha256(canon(manifest)), "summary identity or status differs");
  assert(summary.request_journal.head === completed.previous && summary.request_journal.records === completed.seq, "summary journal anchor mismatch");
  assert(summary.usage.calls === manifest.calls && summary.usage.settled_micro_usd === spent && summary.usage.input_tokens === input && summary.usage.output_tokens === outputTokens, "summary usage mismatch");
  assert(spent <= manifest.cost.run_cap_micro_usd && spent + manifest.cost.prior_project_reserve_micro_usd <= manifest.cost.study_cap_micro_usd, "charge exceeds a cap");
  const finals = read("final-scores.json");
  assert(canon(summary.final_scores) === canon(finals), "summary final scores differ from retained final scores");
  assert(same(Object.keys(finals).sort(), [...SEEDS].sort()), "final repetition roster differs");
  for (const seed of manifest.seeds) {
    assert(finalRepetitions.get(seed)?.final_sha256 === sha256(canon(finals[seed])), "final repetition commitment differs");
    assert(same(Object.keys(finals[seed]).sort(), [...ARMS, "controls"].sort())
      && same(Object.keys(finals[seed].controls).sort(), [...CONTROLS].sort()), "final arm or control roster differs");
    // These are checks of retained bytes, not independent evaluation of the rule.
    for (const row of [finals[seed].independent, finals[seed].feedback, ...Object.values(finals[seed].controls)]) {
      assert(typeof row.source === "string" && sha256(row.source) === row.package, "final package bytes do not match digest");
      assert(row.of === 60 && Array.isArray(row.scores) && row.scores.length === 60
        && row.scores.every((score) => typeof score?.success === "boolean"), "final score panel is incomplete");
      const ratios = row.scores.map((score) => score?.detail?.ratio);
      const invalid = ratios.filter((ratio) => !Number.isFinite(ratio)).length;
      const mean = invalid ? null : ratios.reduce((sum, ratio) => sum + ratio, 0) / ratios.length;
      assert(row.success_count === row.scores.filter((score) => score.success).length
        && row.invalid_count === invalid && row.mean_ratio === mean, "final score aggregates differ from retained outcomes");
    }
    for (const arm of ARMS) {
      const { state, rows } = read(`state-${seed}-${arm}.json`);
      const checked = verifyLedger(state.ledger);
      assert(checked.ok && rows.length === 5 && state.seed === seed && state.lab === "judged_demo", `engine ledger or round count failed: ${seed}/${arm}`);
      for (const [i, row] of rows.entries()) {
        const recorded = rounds.get(`${seed}/${arm}/${row.round}`);
        assert(row.round === i + 1 && recorded, "round binding mismatch");
        const savedRow = Object.fromEntries(Object.entries(recorded).filter(([key]) => !["event", "seq", "previous", "at", "seed", "arm"].includes(key)));
        assert(same(row, savedRow), "persisted round differs from journal");
      }
      assert(rows.at(-1).engine_ledger_head === checked.head, "final engine head mismatch");
      const selected = state.frontier["jobshop-10x5"]?.package || state.packages[sha256("(op) => -op.duration")];
      assert(selected.sha256 === finals[seed][arm].package && selected.source === finals[seed][arm].source
        && finalOpened.winners[seed][arm] === selected.sha256, "final selected package differs from frozen winner");
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
  const mean = differences.reduce((sum, x) => sum + x, 0) / differences.length;
  const variance = differences.reduce((sum, x) => sum + (x - mean) ** 2, 0) / (differences.length - 1);
  const halfWidth = 2.776 * Math.sqrt(variance / differences.length);
  const positive = differences.filter((x) => x > 0).length;
  assert(summary.mean_difference === mean && same(summary.interval_95, [mean - halfWidth, mean + halfWidth])
    && summary.positive_repetitions === positive
    && summary.declared_success === (mean >= 0.05 && mean - halfWidth > 0 && positive >= 4), "summary benefit claim differs from paired outcomes");
}
process.stdout.write(JSON.stringify({ status: summary?.status || "incomplete", events: index, head: previous, requests: pending.size,
  settled: settled.size, spent_usd: spent / 1e6, recomputed_final: process.argv.includes("--recompute-final") }) + "\n");
