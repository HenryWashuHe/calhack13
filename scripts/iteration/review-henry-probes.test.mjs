import test from "node:test";
import assert from "node:assert/strict";
const E = new URL(".", import.meta.url).pathname.replace(/\/$/, "");
const { createState, runGeneration, run, exportBundle, adopt, confirm, verifyLedger, sha256, canon, makePackage } = await import(E + "/engine.mjs");
const { toTrialPlans } = await import(E + "/emit.mjs");
const criterion = { rule: "success-count-margin", min_margin: 1 };
// scripted family: success by table[source](instance); optional one-shot throw
const scripted = (table, opts = {}) => {
  let throws = opts.throwOnce ? 1 : 0;
  return {
    id: opts.id || "scripted", root: () => "root",
    instances: opts.instances || ((seed, n) => Array.from({ length: n }, (_, i) => `s/${seed.slice(0, 8)}/${i}`)),
    score: (pkg, inst) => {
      if (throws && (!opts.throwSource || pkg.source === opts.throwSource)) { throws--; throw new Error("transient"); }
      return { success: table[pkg.source](inst), progress: 0, cost: 1, detail: {} };
    },
  };
};
const pkg = (src) => ({ sha256: sha256(src), source: src, origin: "control", parent: null, generation: 0 });
const T = () => true, F = () => false, half = (i) => Number(i.split("/").at(-1)) % 2 === 0;

test("P1 duplicate control ids: second (stronger) control is never evaluated, candidate promoted", () => {
  const fam = scripted({ root: F, weak: F, strong: T, cand: half });
  const s = createState({ lab: "a", seed: 1 });
  const out = runGeneration(s, { family: fam, mutator: { propose: () => [{ source: "cand" }] }, k: 1, criterion, heldout: 4,
    controls: [{ id: "x", package: pkg("weak") }, { id: "x", package: pkg("strong") }] });
  const evals = s.ledger.records.filter((r) => r.type === "evaluation").map((r) => r.slot + "=" + r.success_count);
  console.log("evaluations:", evals, "promoted:", !!out.promoted);
  assert.equal(out.promoted, null, "candidate (2/4) must not beat a control that scores 4/4");
});

test("P2 adopt while a plan is pending: adoption verdict is a different candidate's", () => {
  const fam = scripted({ root: F, local: T, foreign: F }, { throwOnce: true, throwSource: "local" });
  const s = createState({ lab: "b", seed: 2 });
  assert.throws(() => runGeneration(s, { family: fam, mutator: { propose: () => [{ source: "local" }] }, k: 1, criterion, heldout: 3 }));
  // forge a minimal valid sender bundle for package "foreign" via a real sender run
  const famA = scripted({ root: F, foreign: T });
  const a = createState({ lab: "a", seed: 9 });
  runGeneration(a, { family: famA, mutator: { propose: () => [{ source: "foreign" }] }, k: 1, criterion, heldout: 3 });
  const bundle = exportBundle(a, "scripted");
  const out = adopt(s, { bundle, family: fam, criterion, heldout: 3 });
  const evalForeign = s.ledger.records.some((r) => r.type === "evaluation" && r.package === sha256("foreign"));
  const adoption = s.ledger.records.findLast((r) => r.type === "adoption");
  console.log("adopt verdict:", out.verdict, "| foreign ever evaluated:", evalForeign, "| frontier is foreign:", s.frontier.scripted.package.sha256 === sha256("foreign"), "| adoption record verdict:", adoption.verdict);
  assert.ok(!(out.verdict === "promoted" && !evalForeign), "adoption must not be promoted for a package that was never evaluated");
});

test("P3 run() budget overrun when resuming a larger pending plan", () => {
  const fam = scripted({ root: F, c0: T, c1: T, c2: T, c3: T }, { throwOnce: true });
  const s = createState({ lab: "a", seed: 3 });
  const m = { propose: () => ["c0", "c1", "c2", "c3"].map((source) => ({ source })) };
  assert.throws(() => runGeneration(s, { family: fam, mutator: m, k: 4, criterion, heldout: 6 }));
  const before = s.evaluations, budget = before + 2;
  run(s, { family: fam, mutator: m, k: 1, criterion, heldout: 1, generations: 1, budget: { evaluations: budget } });
  console.log("evaluations before", before, "budget", budget, "after", s.evaluations);
  assert.ok(s.evaluations <= budget, "declared evaluation budget must never be overrun");
});

test("P4 panel-size refusal still spends a mutator call, a generation and registers candidates", () => {
  const fam = scripted({ root: F, c: T }, { instances: (seed, n) => ["only-one"] });
  const s = createState({ lab: "a", seed: 4 });
  const out = runGeneration(s, { family: fam, mutator: { propose: () => [{ source: "c" }] }, k: 1, criterion, heldout: 3 });
  console.log("refused:", out.refused, "| calls", s.calls, "| generation", s.generation, "| records", s.ledger.records.map((r) => r.type));
  assert.equal(s.calls, 0, "a refusal must spend nothing");
});

test("P5 truncation with rewritten head verifies; adopt accepts the truncated bundle", () => {
  const fam = scripted({ root: F, c: T, d: F });
  const s = createState({ lab: "a", seed: 5 });
  runGeneration(s, { family: fam, mutator: { propose: () => [{ source: "c" }] }, k: 1, criterion, heldout: 3 });
  const records = s.ledger.records.slice(0, -1); // drop the last record (promotion)... keep promotion: drop later
  runGeneration(s, { family: fam, mutator: { propose: () => [{ source: "d" }] }, k: 1, criterion, heldout: 3 });
  const full = s.ledger.records, cut = full.slice(0, -1);
  const res = verifyLedger({ records: cut, head: cut.at(-1).sha256 });
  console.log("dropped last record type:", full.at(-1).type, "| verify:", res.ok);
  assert.equal(res.ok, false, "dropping the last record must be detected");
});

test("P6 canon ambiguity: Infinity and the object {$n:'Infinity'} hash identically", () => {
  const fam = scripted({ root: F, c: T });
  const s = createState({ lab: "a", seed: 6 });
  runGeneration(s, { family: { ...fam, score: (p, i) => ({ success: p.source === "c", progress: Infinity, cost: 1 }) }, mutator: { propose: () => [{ source: "c" }] }, k: 1, criterion, heldout: 2 });
  const recs = s.ledger.records.map((r) => JSON.parse(JSON.stringify(r, (k, v) => (v === Infinity ? { $n: "Infinity" } : v))));
  const res = verifyLedger({ records: recs, head: s.ledger.head });
  console.log("canon(Infinity)===canon({$n:'Infinity'}):", canon(Infinity) === canon({ $n: "Infinity" }), "| edited ledger verifies:", res.ok);
  assert.equal(res.ok, false, "changing a number to an object must change the hash");
});

test("P7 confirm after a scorer throw: not once-only, same panel rescored", () => {
  let n = 0;
  const base = scripted({ root: F, c: T });
  const fam = { ...base, score: (p, i) => { if (p.source === "c" && i.startsWith("s/") === false) {} if (++n === 7) throw new Error("flaky"); return base.score(p, i); } };
  const s = createState({ lab: "a", seed: 7 });
  runGeneration(s, { family: fam, mutator: { propose: () => [{ source: "c" }] }, k: 1, criterion, heldout: 3 });
  // 6 evaluations used; the 7th (first confirm score) throws
  assert.throws(() => confirm(s, { family: fam, heldout: 3, panelSeed: "final" }));
  let second;
  try { second = confirm(s, { family: fam, heldout: 3, panelSeed: "final" }); } catch (e) { second = e.message; }
  const declared = s.ledger.records.filter((r) => r.type === "confirmation_declared").length;
  console.log("second confirm:", second, "| confirmation_declared records:", declared, "| evaluations", s.evaluations);
  assert.equal(declared, 1, "a declared confirmation panel must not be re-declared and rescored");
});

test("P8 emit: control ids differing only by case/punctuation collide into one plan id and run ids", () => {
  const plan = { sha256: "a".repeat(64), generation: 1, instances: ["i0"], criterion: { rule: "success-count-margin", min_margin: 1 },
    arms: [{ slot: "incumbent", package: "1".repeat(64) }, { slot: "control:Fixed", package: "2".repeat(64) }, { slot: "control:fixed", package: "3".repeat(64) }, { slot: "candidate:0", package: "4".repeat(64) }] };
  const ref = (id) => ({ domain: "genesis", schema: "x/1", id, version: "1", sha256: sha256(id) });
  const refs = { criterion: ref("c"), model_settings: ref("m"), development_environment: ref("e"), instance: (i) => ref(i), method: (p) => ref("method-" + p.sha256), dependencies: () => [] };
  for (const k of ["public_task", "evaluator", "environment", "generator", "instance_set", "design_schema", "metric_schema", "scoring_rule"]) refs[k] = ref(k);
  const g = { model_calls: 1, input_tokens: 1, output_tokens: 1, wall_time_ms: 1, simulation_steps: 1 };
  const { plans, sources } = toTrialPlans(plan, { refs, grants: { development: g, scoring: g }, org_id: "org_a", id: "t" });
  const ids = plans.map((p) => p.id), runs = plans.flatMap((p) => p.slots.map((s) => s.run_id));
  console.log("plans", plans.length, "unique ids", new Set(ids).size, "unique run ids", new Set(runs).size, "of", runs.length, "| sources keys", Object.keys(sources).length);
  assert.equal(new Set(ids).size, plans.length, "plan ids must be unique per pairing");
});
