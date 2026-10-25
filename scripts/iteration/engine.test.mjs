// Tests for the domain-free iteration engine. The family here is SYNTHETIC (see synthetic-family.mjs):
// it exists only to exercise the protocol and proves nothing about any real task family.
// Run: node --test scripts/iteration/engine.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import { createState, runGeneration, run, exportBundle, adopt, confirm, verifyLedger, sha256 } from "./engine.mjs";
import { syntheticFamily, syntheticMutator, syntheticControls } from "./synthetic-family.mjs";

const criterion = { rule: "success-count-margin", min_margin: 1 };
const setup = (seed = 7, target = 50) => {
  const family = syntheticFamily({ id: "synthetic-line", seed, target, tolerance: 3 });
  const state = createState({ lab: "lab_a", seed });
  return { family, state, controls: syntheticControls(family), mutator: syntheticMutator({ step: 4 }) };
};

test("candidates are proposed before the held-out instances exist, and the plan is frozen before any score", () => {
  const { family, state, controls, mutator } = setup();
  const seen = [];
  const spy = { propose: (args) => { seen.push(args); return mutator.propose(args); } };
  const gen = runGeneration(state, { family, mutator: spy, k: 3, controls, criterion, heldout: 6 });
  assert.equal(seen.length, 1);
  assert.ok(!("instances" in seen[0]), "the mutator never receives instance ids");
  const records = state.ledger.records.map((r) => r.type);
  assert.ok(records.indexOf("candidates_proposed") < records.indexOf("plan_frozen"));
  assert.ok(records.indexOf("plan_frozen") < records.indexOf("evaluation"));
  assert.equal(gen.plan.sha256.length, 64);
  assert.equal(gen.plan.instances.length, 6);
});

test("a candidate is promoted only when it beats every control and the frontier by the declared margin; ties are not promotions", () => {
  const { family, state, controls, mutator } = setup(7, 50);
  // Seed the frontier with a weak package far from the target, so an improvement exists.
  const first = runGeneration(state, { family, mutator, k: 4, controls, criterion, heldout: 6 });
  assert.ok(first.verdicts.every((v) => ["promoted", "rejected"].includes(v.verdict)));
  for (const v of first.verdicts) {
    assert.match(v.reason, /success/, "each verdict states the counts it compared");
    if (v.verdict === "promoted") assert.ok(v.margins.every((m) => m.margin >= criterion.min_margin));
  }
  const promoted = first.verdicts.filter((v) => v.verdict === "promoted");
  assert.ok(promoted.length <= 1, "at most one promotion per generation");
  // Same candidate again cannot be promoted: it ties with the frontier (itself).
  if (promoted.length) {
    const same = { propose: () => [{ source: promoted[0].package.source, note: "identical" }] };
    const again = runGeneration(state, { family, mutator: same, k: 1, controls, criterion, heldout: 6 });
    assert.equal(again.verdicts.length, 0, "a candidate identical to the frontier is dropped before evaluation");
    assert.equal(again.dropped.length, 1);
  }
});

test("a regression is rejected and the frontier stays exactly where it was (today's real outcome)", () => {
  const { family, state, controls } = setup(3, 50);
  const warm = run(state, { family, mutator: syntheticMutator({ step: 4 }), k: 4, controls, criterion, heldout: 6, generations: 6 });
  assert.ok(warm.promotions >= 1, "a frontier exists before the regression is thrown at it");
  const before = state.frontier[family.id].package.sha256;
  const worse = { propose: ({ parent }) => [{ source: JSON.stringify({ guess: JSON.parse(parent.source).guess - 40 }), note: "regression" }] };
  const gen = runGeneration(state, { family, mutator: worse, k: 1, controls, criterion, heldout: 6 });
  assert.equal(gen.verdicts[0].verdict, "rejected");
  assert.equal(state.frontier[family.id].package.sha256, before);
  assert.ok(state.ledger.records.some((r) => r.type === "verdict" && r.verdict === "rejected"), "the loss is on the record");
});

test("the ledger is append-only and chain-hashed; a tampered record is detected", () => {
  const { family, state, controls, mutator } = setup();
  runGeneration(state, { family, mutator, k: 2, controls, criterion, heldout: 4 });
  assert.equal(verifyLedger(state.ledger).ok, true);
  const tampered = structuredClone(state.ledger);
  const ev = tampered.records.find((r) => r.type === "evaluation");
  ev.scores[0].success = !ev.scores[0].success;
  const check = verifyLedger(tampered);
  assert.equal(check.ok, false);
  assert.match(check.reason, /record \d+/);
});

test("the same seed and inputs reproduce the same ledger hashes", () => {
  const a = setup(11, 42), b = setup(11, 42);
  run(a.state, { family: a.family, mutator: a.mutator, k: 3, controls: a.controls, criterion, heldout: 5, generations: 3 });
  run(b.state, { family: b.family, mutator: b.mutator, k: 3, controls: b.controls, criterion, heldout: 5, generations: 3 });
  assert.equal(a.state.ledger.head, b.state.ledger.head);
});

test("the frontier never gets worse and a sensible mutator moves it on the synthetic family", () => {
  const { family, state, controls, mutator } = setup(5, 60);
  const out = run(state, { family, mutator, k: 4, controls, criterion, heldout: 6, generations: 8 });
  const promotions = state.ledger.records.filter((r) => r.type === "promotion");
  for (const p of promotions) assert.ok(p.success_count - p.incumbent_count >= criterion.min_margin, "each promotion beat the incumbent on the same frozen instances by the declared margin");
  assert.ok(out.promotions >= 1, "the synthetic mutator finds at least one verified improvement");
  assert.ok(out.generations <= 8);
});

test("budget and stall rules stop the loop and say why", () => {
  const { family, state, controls, mutator } = setup();
  const byBudget = run(state, { family, mutator, k: 3, controls, criterion, heldout: 4, generations: 50, budget: { evaluations: 20 } });
  assert.equal(byBudget.stopped, "budget");
  const s2 = setup(2, 50);
  const never = { propose: ({ parent }) => [{ source: parent.source + " ", note: "noise" }] };
  const byStall = run(s2.state, { family: s2.family, mutator: never, k: 1, controls: s2.controls, criterion, heldout: 4, generations: 50, stall: 2 });
  assert.equal(byStall.stopped, "stall");
});

test("confirmation scores the frontier on a fresh panel that selection never saw, records it, and changes nothing", () => {
  const { family, state, controls, mutator } = setup(5, 60);
  const out = run(state, { family, mutator, k: 4, controls, criterion, heldout: 6, generations: 6 });
  assert.ok(out.promotions >= 1);
  const before = state.frontier[family.id];
  const c = confirm(state, { family, heldout: 8, panelSeed: "final-2026-10-24" });
  const rec = state.ledger.records.at(-1);
  assert.equal(rec.type, "confirmation");
  assert.equal(c.package, before.package.sha256);
  const selectionSeeds = new Set(state.ledger.records.filter((r) => r.type === "plan_frozen").map((r) => r.instance_seed));
  assert.ok(!selectionSeeds.has(rec.instance_seed), "the confirmation panel is not a selection panel");
  const selectionInstances = new Set(state.ledger.records.filter((r) => r.type === "plan_frozen").flatMap((r) => r.instances));
  assert.ok(rec.instances.every((i) => !selectionInstances.has(i)), "no confirmation instance was ever scored during selection");
  assert.deepEqual(state.frontier[family.id], before, "confirmation never moves the frontier or its feedback");
  assert.throws(() => confirm(state, { family, heldout: 4 }), /panelSeed/);
  assert.equal(verifyLedger(state.ledger).ok, true);
});

test("passing on: a frontier package travels with its lineage, and the adopting family re-verifies on its own instances", () => {
  const a = setup(9, 50);
  run(a.state, { family: a.family, mutator: a.mutator, k: 4, controls: a.controls, criterion, heldout: 6, generations: 6 });
  const bundle = exportBundle(a.state, a.family.id);
  assert.equal(bundle.package.sha256, a.state.frontier[a.family.id].package.sha256);
  assert.ok(bundle.lineage.length >= 1, "ancestors are listed back to the root");
  assert.equal(verifyLedger(bundle.ledger).ok, true);
  assert.equal(bundle.ledger_sha256, verifyLedger(bundle.ledger).head, "the bundle is bound to its ledger's canonical head");
  // Family B has a different target: A's winner is probably wrong for B and must be rejected there.
  const b = setup(21, 120);
  const outcome = adopt(b.state, { bundle, family: b.family, controls: b.controls, criterion, heldout: 6 });
  assert.equal(outcome.verdict, "rejected");
  assert.equal(b.state.frontier[b.family.id], undefined, "nothing is inherited without a local win");
  // Family C shares A's target: A's winner should pass C's own verification and become C's frontier.
  const c = setup(33, 50);
  const ok = adopt(c.state, { bundle, family: c.family, controls: c.controls, criterion, heldout: 6 });
  assert.equal(ok.verdict, "promoted");
  assert.equal(c.state.frontier[c.family.id].package.sha256, bundle.package.sha256);
  assert.equal(c.state.frontier[c.family.id].package.origin, "lab_a", "provenance is kept");
  assert.ok(c.state.ledger.records.some((r) => r.type === "adoption"), "the adoption and its re-verification are on C's ledger");
});

test("a root whose bytes were first seen as another family's control is still registered for its own family, with its control provenance kept", () => {
  const panel = (seed, n) => Array.from({ length: n }, (_, i) => `${seed}/${i}`);
  const shared = "shared-rule";
  const control = { id: "fixed-shared", package: { sha256: sha256(shared), source: shared, origin: "control", parent: null, generation: 0 } };
  const first = { id: "first", root: () => "first-root", instances: panel, score: (pkg) => ({ success: pkg.source === "first-child" }) };
  const second = { id: "second", root: () => shared, instances: panel, score: (pkg, instance) => ({ success: pkg.source === shared || instance.endsWith("/0") }) };
  const state = createState({ lab: "A", seed: "s" });
  runGeneration(state, { family: first, mutator: { propose: () => [{ source: "first-child" }] }, controls: [control], criterion, k: 1, heldout: 2 });
  const out = runGeneration(state, { family: second, mutator: { propose: () => [{ source: "second-child" }] }, criterion, k: 1, heldout: 2 });
  const roots = state.ledger.records.filter((r) => r.type === "root_registered").map((r) => [r.family, r.package]);
  assert.deepEqual(roots, [["first", sha256("first-root")], ["second", sha256(shared)]], "each family registers its root once, even when the bytes were already known");
  assert.equal(state.packages[sha256(shared)].origin, "control", "first provenance wins");
  assert.equal(out.verdicts[0].verdict, "rejected", "the 1/2 child does not beat the 2/2 root");
  assert.ok(verifyLedger(state.ledger).ok);
});


test("a proposal names its real source parent: a root-prompted candidate after a promotion keeps the root's identity; an unknown parent is dropped", () => {
  const panel = (seed, n) => Array.from({ length: n }, (_, i) => `${seed}/${i}`);
  const family = { id: "f", root: () => "root", instances: panel, score: (pkg, instance) => ({ success: pkg.source === "child" || (pkg.source === "grandchild" && instance.endsWith("/0")) }) };
  const state = createState({ lab: "A", seed: "s" });
  const first = runGeneration(state, { family, mutator: { propose: () => [{ source: "child" }] }, criterion, k: 1, heldout: 2 });
  assert.equal(first.verdicts[0].verdict, "promoted");
  const root = sha256("root"), child = sha256("child");
  assert.equal(state.packages[child].parent, root, "the default parent is the incumbent that was handed over");
  const second = runGeneration(state, { family, mutator: { propose: ({ parent }) => { assert.equal(parent.sha256, child); return [{ source: "grandchild", parent: root }, { source: "orphan", parent: "f".repeat(64) }, { source: "sibling" }]; } }, criterion, k: 3, heldout: 2 });
  assert.equal(state.packages[sha256("grandchild")].parent, root, "a root-prompted candidate retains the root as its exact source parent");
  assert.equal(state.packages[sha256("sibling")].parent, child, "an unnamed parent defaults to the incumbent");
  assert.ok(second.dropped.some((d) => d.package === sha256("orphan") && /unknown proposal parent/.test(d.reason)), "an unknown parent is dropped, never guessed");
  const rec = state.ledger.records.find((r) => r.type === "candidates_proposed" && r.generation === 2);
  assert.deepEqual(rec.parents, { [sha256("grandchild")]: root, [sha256("sibling")]: child });
  assert.ok(verifyLedger(state.ledger).ok);
});
