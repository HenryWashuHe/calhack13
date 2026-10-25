// Prospective, paired judged-demo rehearsal. See PROTOCOL.md before using a key.
import fs from "node:fs";
import path from "node:path";
import { spawnSync, execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { canon, createState, runGeneration, sha256, verifyLedger } from "../../scripts/iteration/engine.mjs";
import { jobshopFamily, jobshopControls, featureDoc } from "../../scripts/iteration/families/jobshop.mjs";
import { claudeMutator } from "../../scripts/iteration/mutators/claude.mjs";
import { cascade } from "../../scripts/iteration/cascade.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const CALL = path.join(ROOT, "provider-call.mjs");
const MODEL = "claude-sonnet-5-5";
const EFFORT = "medium";
const MAX_OUTPUT = 8000;
const MAX_BODY_BYTES = 30_000;
const INPUT_RESERVE = 2 * MAX_BODY_BYTES + 2048;
const PER_CALL_MICRO_USD = INPUT_RESERVE * 2 + MAX_OUTPUT * 10;
const SEEDS = Array.from({ length: 5 }, (_, i) => `judge-20261025-${String(i + 1).padStart(2, "0")}`);
const ARMS = ["independent", "feedback"];
const ROUNDS = 5;
const CALL_COUNT = SEEDS.length * ARMS.length * ROUNDS;
const RUN_CAP = 20_000_000;
const STUDY_CAP = 50_000_000;
const TASK = "The task: 10 jobs, 5 machines; every job visits every machine once in its own order, with integer processing times between 1 and 99. A greedy dispatcher runs the shop: whenever a machine is free and has ready operations, it starts the one with the HIGHEST priority value. Success on an instance means makespan <= 1.20 × the larger of the heaviest machine load and the longest job. Progress is lower_bound / makespan. Instances are drawn fresh and hidden every generation.";

function option(name) {
  const i = process.argv.indexOf(`--${name}`);
  return i < 0 ? null : process.argv[i + 1] || null;
}
function required(name) {
  const value = option(name);
  if (!value || value.startsWith("--")) throw new Error(`--${name} is required`);
  return value;
}
function writeNew(file, value) {
  const fd = fs.openSync(file, "wx", 0o600);
  try { fs.writeSync(fd, JSON.stringify(value, null, 2) + "\n"); fs.fsyncSync(fd); }
  finally { fs.closeSync(fd); }
}
function overwrite(file, value) {
  const temporary = `${file}.tmp`;
  const fd = fs.openSync(temporary, "w", 0o600);
  try { fs.writeSync(fd, JSON.stringify(value, null, 2) + "\n"); fs.fsyncSync(fd); }
  finally { fs.closeSync(fd); }
  fs.renameSync(temporary, file);
}
function loadKey(file) {
  const line = fs.readFileSync(file, "utf8").split(/\r?\n/).find((x) => /^GENESIS_LLM_API_KEY\s*=/.test(x));
  if (!line) throw new Error("GENESIS_LLM_API_KEY missing from local env file");
  const raw = line.slice(line.indexOf("=") + 1).trim();
  const key = raw.replace(/^(?:"([\s\S]*)"|'([\s\S]*)')$/, (_, a, b) => a ?? b);
  if (!key || /\s/.test(key)) throw new Error("local API key has an unsupported format");
  return key;
}
class Journal {
  constructor(file) { this.fd = fs.openSync(file, "wx", 0o600); this.file = file; this.seq = 0; this.head = sha256("jobshop-controlled/1"); }
  append(fields) {
    const body = { seq: this.seq++, previous: this.head, at: new Date().toISOString(), ...fields };
    const hash = sha256(canon(body));
    fs.writeSync(this.fd, JSON.stringify({ ...body, hash }) + "\n");
    fs.fsyncSync(this.fd);
    this.head = hash;
    return hash;
  }
  verify() {
    let previous = sha256("jobshop-controlled/1");
    let count = 0;
    for (const line of fs.readFileSync(this.file, "utf8").trim().split("\n")) {
      const { hash, ...body } = JSON.parse(line);
      if (body.seq !== count++ || body.previous !== previous || sha256(canon(body)) !== hash) throw new Error("request journal chain mismatch");
      previous = hash;
    }
    if (previous !== this.head || count !== this.seq) throw new Error("request journal head mismatch");
    return { records: count, head: previous };
  }
  close() { fs.closeSync(this.fd); }
}
function sourceHead() { return execFileSync("git", ["rev-parse", "HEAD"], { cwd: ROOT, encoding: "utf8" }).trim(); }
export function frozenPanels(seed) {
  const family = jobshopFamily({ id: "jobshop-10x5", seed });
  const selection = Array.from({ length: ROUNDS }, (_, i) => {
    const instanceSeed = sha256(canon(["panel", "judged_demo", seed, family.id, i + 1]));
    return sha256(canon(family.instances(instanceSeed, 20)));
  });
  return { selection, final: sha256(canon(family.instances(sha256(`final:${seed}`), 60))) };
}
export function usageCharge(usage) {
  if (!usage || !Number.isSafeInteger(usage.input_tokens) || !Number.isSafeInteger(usage.output_tokens) || usage.input_tokens < 0 || usage.output_tokens < 0) throw new Error("provider usage is unknown");
  if ((usage.cache_creation_input_tokens || 0) !== 0 || (usage.cache_read_input_tokens || 0) !== 0) throw new Error("unexpected cache billing");
  const micro = usage.input_tokens * 2 + usage.output_tokens * 10;
  if (usage.input_tokens > INPUT_RESERVE || usage.output_tokens > MAX_OUTPUT || micro > PER_CALL_MICRO_USD) throw new Error("actual tokens exceeded the reserved request bound");
  return micro;
}
function scorePanel(family, instances, package_) {
  const scores = instances.map((instance) => family.score(package_, instance));
  const ratios = scores.map((x) => x?.detail?.ratio);
  const completeRatios = ratios.every((x) => typeof x === "number" && Number.isFinite(x));
  return { package: package_.sha256, source: package_.source, success_count: scores.filter((x) => x.success).length,
    of: scores.length, invalid_count: scores.filter((x) => !Number.isFinite(x?.detail?.ratio)).length,
    mean_ratio: completeRatios ? ratios.reduce((sum, x) => sum + x, 0) / ratios.length : null,
    scores };
}
function main() {
  const output = path.resolve(required("output-dir"));
  const envFile = path.resolve(required("key-file"));
  const priorUsd = Number(required("prior-project-reserve-usd"));
  if (!Number.isFinite(priorUsd) || priorUsd < 0) throw new Error("prior project reserve must be a verified nonnegative USD bound");
  const priorMicro = Math.ceil(priorUsd * 1_000_000);
  const fullReserve = PER_CALL_MICRO_USD * CALL_COUNT;
  if (fullReserve > RUN_CAP || priorMicro + fullReserve > STUDY_CAP) throw new Error("full predeclared request reserve exceeds a dollar ceiling");
  if (fs.existsSync(output)) throw new Error("output directory already exists");
  const key = loadKey(envFile); // stays in this process memory, not process.env or a result file
  fs.mkdirSync(output, { recursive: false, mode: 0o700 });
  const manifest = {
    schema: "jobshop-controlled/1", source_head: sourceHead(), protocol_sha256: sha256(fs.readFileSync(path.join(ROOT, "PROTOCOL.md"), "utf8")),
    model: MODEL, effort: EFFORT, max_output_tokens: MAX_OUTPUT, fallbacks: "disabled", calls: CALL_COUNT,
    generations: ROUNDS, candidates_kept: 4, candidates_screened: 8, selection_instances: 20, final_instances: 60,
    seeds: SEEDS, panels: Object.fromEntries(SEEDS.map((seed) => [seed, frozenPanels(seed)])),
    cost: { run_cap_micro_usd: RUN_CAP, prior_project_reserve_micro_usd: priorMicro, study_cap_micro_usd: STUDY_CAP,
      per_call_reserve_micro_usd: PER_CALL_MICRO_USD, full_reserve_micro_usd: fullReserve,
      price: "Sonnet 5.5 base USD 2/MTok input, 10/MTok output; no cache/batch/fallback" },
  };
  writeNew(path.join(output, "manifest.json"), manifest);
  const journal = new Journal(path.join(output, "events.ndjson"));
  journal.append({ event: "run_started", manifest_sha256: sha256(canon(manifest)), full_reserve_micro_usd: fullReserve });
  const usage = { calls: 0, settled_micro_usd: 0, input_tokens: 0, output_tokens: 0 };
  let fatal = null;
  function call(req, identity) {
    if (fatal) throw fatal;
    try {
      if (usage.calls >= CALL_COUNT) throw new Error("request count exceeded predeclared total");
      const body = { model: MODEL, max_tokens: MAX_OUTPUT, system: req.system, messages: [{ role: "user", content: req.user }], output_config: { effort: EFFORT } };
      const bytes = Buffer.byteLength(JSON.stringify(body));
      if (req.model !== MODEL || req.max_tokens !== MAX_OUTPUT || req.effort !== EFFORT || bytes > MAX_BODY_BYTES) throw new Error("request does not meet frozen model or size settings");
      const remaining = CALL_COUNT - usage.calls;
      if (usage.settled_micro_usd + remaining * PER_CALL_MICRO_USD > RUN_CAP || priorMicro + usage.settled_micro_usd + remaining * PER_CALL_MICRO_USD > STUDY_CAP) throw new Error("remaining run reserve exceeds a dollar ceiling");
      const index = usage.calls++;
      const request = { ...body, identity };
      writeNew(path.join(output, `request-${String(index).padStart(3, "0")}.json`), request);
      journal.append({ event: "provider_pending", index, identity, request_sha256: sha256(canon(request)), bytes, reserved_micro_usd: PER_CALL_MICRO_USD });
      const result = spawnSync(process.execPath, [CALL], { input: JSON.stringify(req), encoding: "utf8", env: { GENESIS_LLM_API_KEY: key, PATH: process.env.PATH || "" }, timeout: 180_000, maxBuffer: 8 << 20 });
      if (result.error || result.status !== 0) throw new Error(`provider submission outcome unknown: ${result.error?.code || result.status}`);
      const response = JSON.parse(result.stdout);
      writeNew(path.join(output, `response-${String(index).padStart(3, "0")}.json`), response);
      const charge = usageCharge(response.usage);
      if (response.model !== MODEL) throw new Error("provider returned a different model");
      usage.settled_micro_usd += charge;
      usage.input_tokens += response.usage.input_tokens;
      usage.output_tokens += response.usage.output_tokens;
      journal.append({ event: "provider_settled", index, identity, response_sha256: sha256(canon(response)), model: response.model,
        stop_reason: response.stop_reason, usage: response.usage, charge_micro_usd: charge });
      return response;
    } catch (err) {
      fatal = err;
      journal.append({ event: "run_stopped", reason: "provider or cost uncertainty", detail: String(err.message).slice(0, 180),
        calls_submitted: usage.calls, settled_micro_usd: usage.settled_micro_usd });
      throw err;
    }
  }
  const saved = {};
  try {
    for (const [repetition, seed] of SEEDS.entries()) {
      const family = jobshopFamily({ id: "jobshop-10x5", seed });
      const controls = jobshopControls(family, { randomSearch: { seed: `${seed}:rs`, samples: 124, panel: 5 } });
      const order = repetition % 2 === 0 ? ["feedback", "independent"] : ["independent", "feedback"];
      saved[seed] = { controls, arms: {} };
      for (const arm of order) {
        const state = createState({ lab: "judged_demo", seed });
        let context = null;
        const makeInner = () => claudeMutator({ task: TASK, featureDoc, subject: "a dispatching rule for job-shop scheduling", model: MODEL,
          effort: EFFORT, call: (req) => call(req, context), log: (record) => journal.append({ event: "mutator_observation", identity: context, record }) });
        const learned = makeInner();
        const independent = { usage: learned.usage, observe() {}, propose(args) {
          const fresh = makeInner();
          const source = family.root();
          return fresh.propose({ ...args, parent: { source, sha256: sha256(source) }, feedback: null });
        } };
        const mutator = cascade(arm === "feedback" ? learned : independent, { family, screenSeed: `${seed}:screen`, m: 3, factor: 2 });
        const rows = [];
        for (let round = 1; round <= ROUNDS; round++) {
          context = { seed, arm, round };
          const result = runGeneration(state, { family, mutator, k: 4, controls: [], criterion: { rule: "success-count-margin", min_margin: 1 }, heldout: 20 });
          if (fatal) throw fatal;
          if (result.plan && arm === "feedback") {
            const parent = state.packages[result.plan.arms[0].package];
            const parentScores = state.ledger.records.find((x) => x.type === "evaluation" && x.plan === result.plan.sha256 && x.slot === "incumbent")?.scores;
            mutator.observe({ generation: round, parent, parentScores, verdicts: result.verdicts, of: 20 });
          }
          const verified = verifyLedger(state.ledger);
          if (!verified.ok) throw new Error(`engine ledger failed after ${seed}/${arm}/${round}: ${verified.reason}`);
          const row = { round, plan: result.plan?.sha256 || null, promoted: result.promoted?.sha256 || null, candidates: result.verdicts.map((v) => ({ package: v.package.sha256, success_count: v.success_count, verdict: v.verdict })),
            engine_ledger_head: verified.head, engine_evaluations: state.evaluations, screening_evaluations: mutator.counts.evaluations,
            provider_calls: usage.calls, spent_micro_usd: usage.settled_micro_usd };
          rows.push(row);
          overwrite(path.join(output, `state-${seed}-${arm}.json`), { state, rows });
          journal.append({ event: "round_completed", seed, arm, ...row });
          journal.verify();
        }
        saved[seed].arms[arm] = { state, rows, screening: mutator.counts };
      }
    }
    if (usage.calls !== CALL_COUNT) throw new Error(`incomplete model call count: ${usage.calls}/${CALL_COUNT}`);
    const winners = Object.fromEntries(SEEDS.map((seed) => [seed, Object.fromEntries(ARMS.map((arm) => {
      const { state } = saved[seed].arms[arm];
      const pkg = state.frontier["jobshop-10x5"]?.package || state.packages[sha256("(op) => -op.duration")];
      return [arm, pkg.sha256];
    }))]));
    journal.verify();
    journal.append({ event: "final_panel_opened", winners, calls: usage.calls, settled_micro_usd: usage.settled_micro_usd });
    const finals = {};
    for (const seed of SEEDS) {
      const family = jobshopFamily({ id: "jobshop-10x5", seed });
      const instances = family.instances(sha256(`final:${seed}`), 60);
      finals[seed] = {};
      for (const arm of ARMS) {
        const { state } = saved[seed].arms[arm];
        const pkg = state.frontier[family.id]?.package || state.packages[sha256(family.root())];
        finals[seed][arm] = scorePanel(family, instances, pkg);
      }
      finals[seed].controls = Object.fromEntries(saved[seed].controls.map((control) => [control.id, scorePanel(family, instances, control.package)]));
      overwrite(path.join(output, "final-scores.json"), finals);
      journal.append({ event: "final_repetition_completed", seed, final_sha256: sha256(canon(finals[seed])) });
    }
    const differences = SEEDS.map((seed) => (finals[seed].feedback.success_count - finals[seed].independent.success_count) / 60);
    const mean = differences.reduce((a, b) => a + b, 0) / differences.length;
    const variance = differences.reduce((sum, x) => sum + (x - mean) ** 2, 0) / (differences.length - 1);
    const halfWidth = 2.776 * Math.sqrt(variance / differences.length);
    const result = { status: "complete", manifest_sha256: sha256(canon(manifest)), source_head: manifest.source_head,
      usage, request_journal: journal.verify(), differences, mean_difference: mean,
      interval_95: [mean - halfWidth, mean + halfWidth], positive_repetitions: differences.filter((x) => x > 0).length,
      declared_success: mean >= 0.05 && mean - halfWidth > 0 && differences.filter((x) => x > 0).length >= 4,
      final_scores: finals, limitations: ["single synthetic task family", "five paired repetitions", "operator-local evidence", "not G4 or A-to-B-to-A"] };
    writeNew(path.join(output, "summary.json"), result);
    journal.append({ event: "run_completed", summary_sha256: sha256(canon(result)), spent_micro_usd: usage.settled_micro_usd });
    process.stdout.write(JSON.stringify({ status: result.status, spent_usd: usage.settled_micro_usd / 1e6,
      mean_difference: mean, interval_95: result.interval_95, declared_success: result.declared_success, output }) + "\n");
  } catch (err) {
    if (!fatal) journal.append({ event: "run_stopped", reason: String(err.message).slice(0, 180), calls_submitted: usage.calls, settled_micro_usd: usage.settled_micro_usd });
    process.stderr.write(`run stopped: ${String(err.message).slice(0, 180)}\n`);
    process.exitCode = 1;
  } finally { journal.close(); }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
