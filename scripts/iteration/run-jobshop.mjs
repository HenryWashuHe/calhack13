// The first real run of the iteration engine: job-shop dispatching rules, a model mutator behind a cascade,
// fixed-rule and seeded random-search controls, sealed panels, a final fresh-panel confirmation, and the
// sponsor exports. Everything it prints is also written to --out as JSON so the report can cite it.
//   node scripts/iteration/run-jobshop.mjs --mock                  # no model calls, a local stand-in mutator
//   node scripts/iteration/run-jobshop.mjs --generations 5 --k 4   # live model calls (GENESIS_LLM_API_KEY)
//   --gate all (default): promotion must beat incumbent and every control; --gate incumbent: controls only on the final panel
import fs from "node:fs";
import path from "node:path";
import { createState, runGeneration, confirm, exportBundle, verifyLedger, sha256 } from "./engine.mjs";
import { jobshopFamily, jobshopControls, featureDoc } from "./families/jobshop.mjs";
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

const TASK = `The task: 10 jobs, 5 machines; every job visits every machine once in its own order, with integer processing times between 1 and 99. A greedy dispatcher runs the shop: whenever a machine is free and has ready operations, it starts the one with the HIGHEST priority value. Success on an instance means makespan <= target × lower bound, where the lower bound is the larger of the heaviest machine load and the longest job. Progress is lower_bound / makespan. Instances are drawn fresh and hidden every generation.`;

function mockMutator() { // a local stand-in for dry runs: seeded linear rules, no model
  let n = 0;
  return { usage: { calls: 0, input_tokens: 0, output_tokens: 0, failures: 0 }, observe() {}, propose: ({ k }) => Array.from({ length: k }, () => { const w = [1, 2, 3, 4].map((i) => ((parseInt(sha256(`mock:${n++}:${i}`).slice(0, 6), 16) % 200) - 100) / 100); return { source: `(op) => (${w[0]} * op.duration) + (${w[1]} * op.remainingWork) + (${w[2]} * op.remainingOps) + (${w[3]} * op.machineRemainingLoad)`, note: "mock linear rule" }; }) };
}

function reportPanel(family, instances, arms) { // report-only: every arm on the same final panel, outside selection
  return arms.map(({ id, pkg }) => { const rows = instances.map((i) => family.score(pkg, i)); return { id, package: pkg.sha256, success_count: rows.filter((r) => r.success).length, of: rows.length, mean_ratio: rows.reduce((a, r) => a + ((r.detail && r.detail.ratio) || 0), 0) / rows.length }; });
}

function main() {
  loadEnv();
  const cfg = { generations: num("generations", 5), k: num("k", 4), heldout: num("heldout", 20), screen: num("screen", 3), factor: num("factor", 2), target: num("target", 1.2), seed: String(arg("seed", "smoke-1")), stall: num("stall", 3), mock: arg("mock", false) === true, effort: String(arg("effort", "medium")), gate: String(arg("gate", "all")), model: process.env.GENESIS_LLM_MODEL || "claude-sonnet-5-5" };
  const out = String(arg("out", path.join("scripts", "iteration", "results", `jobshop-${cfg.seed}${cfg.mock ? "-mock" : ""}`)));
  fs.mkdirSync(out, { recursive: true });
  const family = jobshopFamily({ id: "jobshop-10x5", seed: cfg.seed, target: cfg.target });
  const panel = 6, samples = Math.max(8, Math.ceil((cfg.k * cfg.heldout * cfg.generations) / panel)); // random search gets the loop's selection budget
  const controls = jobshopControls(family, { randomSearch: { seed: cfg.seed + ":rs", samples, panel } });
  const calls = [];
  const inner = cfg.mock ? mockMutator() : claudeMutator({ task: TASK, featureDoc, model: cfg.model, effort: cfg.effort, log: (e) => calls.push(e) });
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
    result.final_panel_comparison = { note: "report-only: controls and the root scored on the frontier's confirmation panel, outside the engine's selection", rows: reportPanel(family, declared.instances, [{ id: "frontier", pkg: frontier.package }, { id: "root (SPT)", pkg: state.packages[sha256(family.root())] }, ...controls.map((c) => ({ id: c.id, pkg: c.package }))]) };
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
