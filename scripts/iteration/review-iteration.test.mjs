import test from "node:test";
import assert from "node:assert/strict";
// Henry's Claude: adversarial regressions for #144 at bae6f50. Each fails on that head.
// Synthetic/scripted families only; nothing real runs.
const E = new URL(".", import.meta.url).href.replace(/\/$/, "");
const { createState, runGeneration, run, exportBundle, adopt, confirm, verifyLedger, sha256, makePackage } = await import(E + "/engine.mjs");
const { syntheticFamily, syntheticMutator, syntheticControls } = await import(E + "/synthetic-family.mjs");
const criterion = { rule: "success-count-margin", min_margin: 1 };
const setup = (seed = 7, target = 50, lab = "lab_a") => {
  const family = syntheticFamily({ id: "synthetic-line", seed, target, tolerance: 3 });
  return { family, state: createState({ lab, seed }), controls: syntheticControls(family), mutator: syntheticMutator({ step: 4 }) };
};
// Tiny scripted family: success decided by a table keyed on package source.
const scripted = (table, { pool = null } = {}) => ({
  id: "scripted", root: () => "root",
  instances: (seed, n) => pool ? pool.slice(0, n) : Array.from({ length: n }, (_, i) => `s/${seed.slice(0, 8)}/${i}`),
  score: (pkg, inst) => ({ success: table[pkg.source](inst), progress: 0, cost: 1, detail: {} }),
});
const fixed = (src) => ({ propose: () => [{ source: src }] });

test("D1 truncated ledger verifies (tail records silently dropped, head field ignored)", () => {
  const { family, state, controls, mutator } = setup();
  run(state, { family, mutator, k: 3, controls, criterion, heldout: 4, generations: 3 });
  const cut = { records: state.ledger.records.slice(0, -5), head: state.ledger.head };
  assert.equal(verifyLedger(cut).ok, false, "dropping the last 5 records (verdicts, run_stopped) must be detected");
});

test("D2 adopt: lineage, exported_by and package provenance are not bound to the verified ledger", () => {
  const a = setup(9, 50);
  run(a.state, { family: a.family, mutator: a.mutator, k: 4, controls: a.controls, criterion, heldout: 6, generations: 6 });
  const bundle = exportBundle(a.state, a.family.id);
  const forged = { ...bundle, exported_by: "lab_z", lineage: [{ sha256: "f".repeat(64) }], package: { ...bundle.package, origin: "lab_z", parent: "e".repeat(64) } };
  const c = setup(33, 50, "lab_c");
  const out = adopt(c.state, { bundle: forged, family: c.family, controls: c.controls, criterion, heldout: 6 });
  assert.notEqual(out.verdict, "promoted", "a bundle whose lineage/sender/provenance are not in its ledger must not be adopted as provenance-kept");
});

test("D3 adopt overwrites first provenance, even when the adoption is rejected", () => {
  const a = setup(9, 50);
  run(a.state, { family: a.family, mutator: a.mutator, k: 4, controls: a.controls, criterion, heldout: 6, generations: 6 });
  const bundle = exportBundle(a.state, a.family.id);
  const c = setup(33, 50, "lab_c");
  // C independently knows the same bytes first, as its own frontier.
  runGeneration(c.state, { family: c.family, mutator: fixed(bundle.package.source), k: 1, controls: c.controls, criterion, heldout: 6 });
  const before = structuredClone(c.state.packages[bundle.package.sha256]);
  assert.equal(before.origin, "lab_c");
  const out = adopt(c.state, { bundle, family: c.family, controls: c.controls, criterion, heldout: 6 });
  assert.equal(out.verdict, "rejected");
  assert.deepEqual(c.state.packages[bundle.package.sha256], before, "first provenance wins");
});

test("D4 adopt verifies on the sender's own selection panels when seeds coincide (instance seed lacks lab identity)", () => {
  const a = setup(9, 50, "lab_a");
  run(a.state, { family: a.family, mutator: a.mutator, k: 4, controls: a.controls, criterion, heldout: 6, generations: 6 });
  const bundle = exportBundle(a.state, a.family.id);
  const senderPanels = new Set(bundle.ledger.records.filter((r) => r.type === "plan_frozen").flatMap((r) => r.instances));
  const b = setup(9, 50, "lab_b");
  adopt(b.state, { bundle, family: b.family, controls: b.controls, criterion, heldout: 6 });
  const verifyPanel = b.state.ledger.records.find((r) => r.type === "plan_frozen").instances;
  assert.ok(verifyPanel.every((i) => !senderPanels.has(i)), "re-verification must not reuse instances the sender selected on");
});

test("D5 confirm panel can be reused across frontiers (confirm, select more, confirm again on same panel)", () => {
  const { family, state, controls, mutator } = setup(5, 60);
  run(state, { family, mutator, k: 4, controls, criterion, heldout: 6, generations: 2 });
  confirm(state, { family, heldout: 8, panelSeed: "final" });
  run(state, { family, mutator, k: 4, controls, criterion, heldout: 6, generations: 3 });
  assert.throws(() => confirm(state, { family, heldout: 8, panelSeed: "final" }), /already used/);
});

test("D6 confirm guard is seed-only: a family whose panels overlap passes as 'fresh'", () => {
  const pool = Array.from({ length: 20 }, (_, i) => `p${i}`);
  const fam = scripted({ root: () => false, good: () => true }, { pool });
  const s = createState({ lab: "x", seed: 1 });
  runGeneration(s, { family: fam, mutator: fixed("good"), k: 1, criterion, heldout: 6 });
  assert.throws(() => confirm(s, { family: fam, heldout: 6, panelSeed: "fresh" }), /already used/, "confirmation instances p0..p5 were all selection instances");
});

test("D7 a string 'false' / any truthy score counts as success", () => {
  const fam = scripted({ root: () => false, cand: () => "false" });
  const s = createState({ lab: "x", seed: 1 });
  const g = runGeneration(s, { family: fam, mutator: fixed("cand"), k: 1, criterion, heldout: 4 });
  assert.equal(g.promoted, null, "success:'false' must not be counted as a success");
});

test("D8 unknown incumbent score (undefined) is counted as a failure and the candidate is promoted", () => {
  const fam = scripted({ root: () => undefined, cand: (i) => i.endsWith("/0") });
  const s = createState({ lab: "x", seed: 1 });
  const g = runGeneration(s, { family: fam, mutator: fixed("cand"), k: 1, criterion, heldout: 4 });
  assert.equal(g.promoted, null, "an unknown outcome must be insufficient, never a verdict (cf. scoring/src/verdict.rs Insufficient)");
});

test("D9 mutator receives instance ids via feedback, contrary to the contract", () => {
  const { family, state, controls, mutator } = setup();
  const seen = [];
  const spy = { propose: (a) => { seen.push(a); return mutator.propose(a); } };
  run(state, { family, mutator: spy, k: 3, controls, criterion, heldout: 4, generations: 2 });
  const ids = JSON.stringify(seen.map((a) => a.feedback));
  assert.ok(!/synthetic-line\/[0-9a-f]{8}\/\d/.test(ids), "mutator saw instance ids: " + ids.slice(0, 120));
});

test("D10 negative k overspends the evaluation budget", () => {
  const { family, state, controls } = setup();
  const six = { propose: ({ parent }) => [1, 2, 3, 4, 5, 6].map((d) => ({ source: JSON.stringify({ guess: JSON.parse(parent.source).guess + d }) })) };
  run(state, { family, mutator: six, k: -1, controls, criterion, heldout: 4, generations: 1, budget: { evaluations: 20 } });
  assert.ok(state.evaluations <= 20, `spent ${state.evaluations} of 20`);
});

test("D11 family returning more instances than heldout overspends the budget", () => {
  const { family, state, controls, mutator } = setup();
  const greedy = { ...family, instances: (seed, n) => family.instances(seed, n * 3) };
  run(state, { family: greedy, mutator, k: 1, controls, criterion, heldout: 4, generations: 1, budget: { evaluations: 16 } });
  assert.ok(state.evaluations <= 16, `spent ${state.evaluations} of 16`);
});

test("D12 bundle hash is key-order dependent: a key-sorted re-serialization verifies but is rejected", () => {
  const a = setup(9, 50);
  run(a.state, { family: a.family, mutator: a.mutator, k: 4, controls: a.controls, criterion, heldout: 6, generations: 6 });
  const bundle = exportBundle(a.state, a.family.id);
  const sortKeys = (x) => Array.isArray(x) ? x.map(sortKeys) : x && typeof x === "object" ? Object.fromEntries(Object.keys(x).sort().map((k) => [k, sortKeys(x[k])])) : x;
  const resorted = { ...bundle, ledger: { ...bundle.ledger, records: sortKeys(bundle.ledger.records) } };
  assert.equal(verifyLedger(resorted.ledger).ok, true);
  const c = setup(33, 50, "lab_c");
  const out = adopt(c.state, { bundle: resorted, family: c.family, controls: c.controls, criterion, heldout: 6 });
  assert.notEqual(out.reason, "bundle ledger does not verify", "semantically identical ledger rejected");
});

test("D13 non-finite/-0 numbers collapse in the canonical hash: Infinity edited to null (or -0 to 0) is undetected", () => {
  const fam = scripted({ root: () => false, cand: () => true });
  fam.score = (pkg, inst) => ({ success: pkg.source === "cand", progress: Infinity, cost: -0, detail: {} });
  const s = createState({ lab: "x", seed: 1 });
  runGeneration(s, { family: fam, mutator: fixed("cand"), k: 1, criterion, heldout: 2 });
  const t = structuredClone(s.ledger);
  const ev = t.records.find((r) => r.type === "evaluation");
  ev.scores[0].progress = null; ev.scores[0].cost = 0;
  assert.equal(verifyLedger(t).ok, false, "a changed score value must break the chain");
});

test("D14 package-hash rejection in adopt leaves no ledger record", () => {
  const a = setup(9, 50);
  run(a.state, { family: a.family, mutator: a.mutator, k: 4, controls: a.controls, criterion, heldout: 6, generations: 6 });
  const bundle = exportBundle(a.state, a.family.id);
  const bad = { ...bundle, package: { ...bundle.package, source: '{"guess":999}' } };
  const c = setup(33, 50, "lab_c");
  const out = adopt(c.state, { bundle: bad, family: c.family, controls: c.controls, criterion, heldout: 6 });
  assert.equal(out.verdict, "rejected");
  assert.ok(c.state.ledger.records.some((r) => r.type === "adoption"), "the declined adoption must be on the adopter's ledger");
});

test("D15 score throwing mid-generation leaves a frozen plan with partial evaluations and no verdict; the retry draws a new panel", () => {
  let fail = true;
  const fam = scripted({ root: () => false, cand: (i) => { if (fail && i.endsWith("/2")) throw new Error("provider down"); return true; } });
  const s = createState({ lab: "x", seed: 1 });
  assert.throws(() => runGeneration(s, { family: fam, mutator: fixed("cand"), k: 1, criterion, heldout: 4 }));
  fail = false;
  runGeneration(s, { family: fam, mutator: fixed("cand"), k: 1, criterion, heldout: 4 });
  const plans = s.ledger.records.filter((r) => r.type === "plan_frozen");
  assert.equal(plans.length, 1, "an abandoned frozen plan must be recorded, not silently replaced by a rerun on a fresh panel");
});

test("D16 exported lineage stops before the root (root never stored in state.packages)", () => {
  const a = setup(5, 60);
  run(a.state, { family: a.family, mutator: a.mutator, k: 4, controls: a.controls, criterion, heldout: 6, generations: 6 });
  const bundle = exportBundle(a.state, a.family.id);
  assert.equal(bundle.lineage.at(-1).sha256, sha256(a.family.root()), "lineage must reach the root");
});
