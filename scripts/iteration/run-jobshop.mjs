// The first real runs of the iteration engine: a task family (--family jobshop | binpack), a model mutator behind a cascade,
// fixed-rule and seeded random-search controls, sealed panels, a final fresh-panel confirmation, and the
// sponsor exports. Everything it prints is also written to --out as JSON so the report can cite it.
//   node scripts/iteration/run-jobshop.mjs --mock                  # no model calls, a local stand-in mutator
//   node scripts/iteration/run-jobshop.mjs --generations 5 --k 4   # live model calls (GENESIS_LLM_API_KEY)
//   --gate all (default): promotion must beat incumbent and every control; --gate incumbent: controls only on the final panel
import fs from "node:fs";
import path from "node:path";
import { createState, runGeneration, confirm, exportBundle, verifyLedger, sha256 } from "./engine.mjs";
import { jobshopFamily, jobshopControls, featureDoc as jobshopDoc, FEATURES as jobshopFeatures, linearSource as jobshopLinear } from "./families/jobshop.mjs";
import { binpackFamily, binpackControls, featureDoc as binpackDoc, FEATURES as binpackFeatures, linearSource as binpackLinear } from "./families/binpack.mjs";
import { claudeMutator } from "./mutators/claude.mjs";
import { cascade } from "./cascade.mjs";
import { toBulk, post } from "./sponsors/elastic.mjs";
import { toOffer, toChatText, toObservation } from "./sponsors/fetch-offer.mjs";

const arg = (name, dflt) => { const i = process.argv.indexOf("--" + name); return i < 0 ? dflt : process.argv[i + 1] === undefined || process.argv[i + 1].startsWith("--") ? true : process.argv[i + 1]; };
const num = (name, dflt) => Number(arg(name, dflt));

function loadEnv(file = ".env") { // only fills variables that are unset; never prints a value
  if (!fs.existsSync(file)) return;
  for (const line of fs.readFileSync(file, "utf8").split("\n")) {
    const m = line.match(/^\s*([A-Z0-9_]+)\s*=\s*(.*)\s*$/); if (!m || process.env[m[1]] !== undefined) continue;
    process.env[m[1]] = m[2].replace(/^["']|["']$/g, "");
  }
}

function familySpec(name) {
  const specs = {
  jobshop: () => ({ id: "jobshop-10x5", make: jobshopFamily, controls: jobshopControls, featureDoc: () => jobshopDoc, linear: () => [jobshopFeatures, jobshopLinear], target: 1.2, root: "SPT", subject: "a dispatching rule for job-shop scheduling",
    task: `The task: 10 jobs, 5 machines; every job visits every machine once in its own order, with integer processing times between 1 and 99. A greedy dispatcher runs the shop: whenever a machine is free and has ready operations, it starts the one with the HIGHEST priority value. Success on an instance means makespan <= target × lower bound, where the lower bound is the larger of the heaviest machine load and the longest job. Progress is lower_bound / makespan. Instances are drawn fresh and hidden every generation.` }),
  binpack: () => ({ id: "binpack-100", make: binpackFamily, controls: binpackControls, featureDoc: () => binpackDoc, linear: () => [binpackFeatures, binpackLinear], target: undefined, root: "first-fit", subject: "a bin-choice rule for online bin packing",
    task: `The task: 100 items with integer sizes in the range given below arrive one at a time; bins have capacity 100. For each item, every open bin with enough room is a candidate; the rule scores each candidate and the item goes into the HIGHEST-scoring bin; if no open bin has room, a new bin is opened. Success on an instance means bins used <= target × lower bound, where the lower bound is ceil(total size / capacity). Progress is lower_bound / bins. Instances are drawn fresh and hidden every generation.` }),
  };
  return specs[name] ? specs[name]() : null;
}

function mockMutator(spec) { // a local stand-in for dry runs: seeded linear rules from the family's own feature list, no model
  let n = 0;
  return { usage: { calls: 0, input_tokens: 0, output_tokens: 0, failures: 0 }, observe() {}, propose: ({ k }) => { const [features, linear] = spec.linear(); return Array.from({ length: k }, () => ({ source: linear(features.map((_, i) => ((parseInt(sha256(`mock:${n++}:${i}`).slice(0, 6), 16) % 200) - 100) / 100)), note: "mock linear rule" })); } };
}

function reportPanel(family, instances, arms) { // report-only: every arm on the same final panel, outside selection
  return arms.map(({ id, pkg }) => { const rows = instances.map((i) => family.score(pkg, i)); return { id, package: pkg.sha256, success_count: rows.filter((r) => r.success).length, of: rows.length, mean_ratio: rows.reduce((a, r) => a + ((r.detail && r.detail.ratio) || 0), 0) / rows.length }; });
}

function main() {
  loadEnv();
  const spec = familySpec(String(arg("family", "jobshop")));
  if (!spec) throw new Error("--family must be one of jobshop, binpack");
  const cfg = { family: spec.id, generations: num("generations", 5), k: num("k", 4), heldout: num("heldout", 20), screen: num("screen", 3), factor: num("factor", 2), target: arg("target", undefined) === undefined ? spec.target : num("target"), seed: String(arg("seed", "smoke-1")), stall: num("stall", 3), mock: arg("mock", false) === true, effort: String(arg("effort", "medium")), gate: String(arg("gate", "all")), model: process.env.GENESIS_LLM_MODEL || "claude-sonnet-5-5" };
  const out = String(arg("out", path.join("scripts", "iteration", "results", `${String(arg("family", "jobshop"))}-${cfg.seed}${cfg.mock ? "-mock" : ""}`)));
  // A run directory is evidence: an occupied one is refused before any work or write, never overwritten.
  if (fs.existsSync(out) && fs.readdirSync(out).length) throw new Error(`refusing to reuse the occupied run directory ${out}; choose a new --seed or --out`);
  fs.mkdirSync(out, { recursive: true });
  const sizes = arg("sizes", undefined) === undefined ? undefined : String(arg("sizes")).split(",").map(Number); // binpack only: item size range, e.g. --sizes 20,70
  cfg.sizes = sizes || null;
  const family = spec.make({ id: spec.id + (sizes ? `-${sizes.join("to")}` : ""), seed: cfg.seed, ...(cfg.target === undefined ? {} : { target: cfg.target }), ...(sizes ? { sizes } : {}) });
  const panel = 6, samples = Math.max(8, Math.ceil((cfg.k * cfg.heldout * cfg.generations) / panel)); // random search gets the loop's selection budget
  const controls = spec.controls(family, { randomSearch: { seed: cfg.seed + ":rs", samples, panel } });
  const calls = [];
  const task = spec.task + (sizes ? ` Item sizes are integers in [${sizes[0]}, ${sizes[1]}].` : spec.id.startsWith("binpack") ? " Item sizes are integers in [1, 50]." : "");
  const inner = cfg.mock ? mockMutator(spec) : claudeMutator({ subject: spec.subject, task, featureDoc: spec.featureDoc(), model: cfg.model, effort: cfg.effort, log: (e) => calls.push(e) });
  const mutator = cascade(inner, { family, screenSeed: cfg.seed + ":screen", m: cfg.screen, factor: cfg.factor });
  const criterion = { rule: "success-count-margin", min_margin: 1 };
  const state = createState({ lab: "lab_a", seed: cfg.seed });
  const generations = [];
  let sinceLast = 0;
  for (let g = 0; g < cfg.generations; g++) {
    // gate "all": a candidate must beat the incumbent and every control on the sealed panel (the contract's default).
    // gate "incumbent": selection is against the incumbent only; controls are compared on the final fresh panel, report-only.
    const r = runGeneration(state, { family, mutator, k: cfg.k, controls: cfg.gate === "incumbent" ? [] : controls, criterion, heldout: cfg.heldout });
    if (r.refused || !r.plan) { generations.push({ generation: state.generation, refused: r.refused || "no candidates kept", dropped: r.dropped }); if (++sinceLast >= cfg.stall) break; continue; }
    const arm = (slot) => state.ledger.records.find((x) => x.type === "evaluation" && x.plan === r.plan.sha256 && x.slot === slot);
    const parent = state.packages[r.plan.arms[0].package];
    mutator.observe({ generation: r.plan.generation, parent, parentScores: arm("incumbent").scores, verdicts: r.verdicts, of: r.plan.instances.length });
    const row = { generation: r.plan.generation, plan: r.plan.sha256, incumbent: arm("incumbent").success_count, controls: r.plan.arms.filter((a) => a.slot.startsWith("control:")).map((a) => ({ slot: a.slot, success_count: arm(a.slot).success_count })), candidates: r.verdicts.map((v) => ({ slot: v.slot, success_count: v.success_count, verdict: v.verdict })), of: r.plan.instances.length, promoted: r.promoted ? r.promoted.sha256 : null, dropped: r.dropped };
    generations.push(row);
    process.stdout.write(`gen ${row.generation}: incumbent ${row.incumbent}/${row.of}; controls ${row.controls.map((c) => c.success_count).join("/")}; candidates ${row.candidates.map((c) => c.success_count + (c.verdict === "promoted" ? "*" : "")).join(" ")}\n`);
    if (r.promoted) sinceLast = 0; else if (++sinceLast >= cfg.stall) { process.stdout.write("stall\n"); break; }
  }
  const frontier = state.frontier[family.id] || null;
  const result = { config: cfg, controls: controls.map((c) => ({ id: c.id, package: c.package.sha256, source: c.package.source, evidence: c.evidence || null })), generations, calls, usage: inner.usage, screening: mutator.counts, evaluations: state.evaluations, model_calls: state.calls };
  if (frontier) {
    const conf = confirm(state, { family, heldout: cfg.heldout, panelSeed: cfg.seed + ":final" });
    const declared = state.ledger.records.find((x) => x.type === "confirmation_declared" && x.package === frontier.package.sha256);
    result.confirmation = conf;
    result.final_panel_comparison = { note: "report-only: controls and the root scored on the frontier's confirmation panel, outside the engine's selection", rows: reportPanel(family, declared.instances, [{ id: "frontier", pkg: frontier.package }, { id: `root (${spec.root})`, pkg: state.packages[sha256(family.root())] }, ...controls.map((c) => ({ id: c.id, pkg: c.package }))]) };
    const bundle = exportBundle(state, family.id);
    fs.writeFileSync(path.join(out, "bundle.json"), JSON.stringify(bundle, null, 1));
    const offer = toOffer(bundle, { from: "lab_a", bundle_ref: path.join(out, "bundle.json") });
    fs.writeFileSync(path.join(out, "fetch-offer.txt"), toChatText(offer));
    fs.writeFileSync(path.join(out, "fetch-observation.json"), JSON.stringify(toObservation(offer), null, 1));
    result.frontier = { package: frontier.package.sha256, source: frontier.package.source, generation: frontier.generation, selection_success_count: frontier.success_count };
  } else result.confirmation = null;
  result.ledger_verify = verifyLedger(state.ledger);
  const bulk = toBulk(state.ledger, { exported_at: new Date().toISOString() });
  fs.writeFileSync(path.join(out, "elastic-bulk.ndjson"), bulk.body);
  fs.writeFileSync(path.join(out, "ledger.json"), JSON.stringify(state.ledger, null, 1));
  fs.writeFileSync(path.join(out, "result.json"), JSON.stringify(result, null, 1));
  return post(bulk).then((es) => { result.elastic = es; fs.writeFileSync(path.join(out, "result.json"), JSON.stringify(result, null, 1)); process.stdout.write(`${JSON.stringify({ frontier: result.frontier && result.frontier.selection_success_count, confirmation: result.confirmation, final_panel: result.final_panel_comparison && result.final_panel_comparison.rows.map((r) => `${r.id} ${r.success_count}/${r.of}`), evaluations: state.evaluations, screening: mutator.counts.evaluations, usage: inner.usage, elastic: es, ledger: { records: state.ledger.records.length, head: state.ledger.head, ok: result.ledger_verify.ok }, out }, null, 1)}\n`); });
}

main().catch((err) => { process.stderr.write(`run failed: ${err && err.stack ? err.stack : err}\n`); process.exit(1); });
