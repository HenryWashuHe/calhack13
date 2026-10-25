// Independent #144 guards: inert labels and finite callbacks only.
// No generated source, subprocess, provider, lab runtime or evaluator executes.
import test from "node:test";
import assert from "node:assert/strict";
import { adopt, createState, exportBundle, makePackage, runGeneration, sha256, verifyLedger } from "./engine.mjs";

const criterion = { rule: "success-count-margin", min_margin: 1 };
const panel = (seed, n) => Array.from({ length: n }, (_, i) => `${seed}/${i}`);
const propose = (source) => ({ propose: () => [{ source }] });
const step = (state, family, source, controls = []) => runGeneration(state, {
  family, mutator: propose(source), controls, criterion, k: 1, heldout: 2,
});
const rootEvents = (state, family, source) => state.ledger.records.filter((r) =>
  r.type === "root_registered" && r.family === family && r.package === sha256(source));

function simpleFamily(id, root, winner) {
  return { id, root: () => root, instances: panel,
    score: (pkg) => ({ success: pkg.source === winner }) };
}

test("a root first seen as another family's control is recorded once and its descendant transfers", () => {
  const state = createState({ lab: "A", seed: "control-root" });
  const control = makePackage("shared-root", { origin: "frozen-control", generation: 0 });
  step(state, simpleFamily("warmup", "warmup-root", "warmup-win"), "warmup-win",
    [{ id: "shared", package: control }]);
  const first = state.packages[control.sha256];
  const family = simpleFamily("new-family", "shared-root", "new-win");
  assert.equal(step(state, family, "new-loss").verdicts[0].verdict, "rejected");
  assert.equal(step(state, family, "new-win").verdicts[0].verdict, "promoted");
  const bundle = exportBundle(state, family.id);
  assert.equal(verifyLedger(bundle.ledger).ok, true);
  const receiver = createState({ lab: "B", seed: "own-panel" });
  const transferred = adopt(receiver, { bundle, family, criterion, heldout: 2 });
  assert.deepEqual({ registrations: rootEvents(state, family.id, "shared-root").length,
    transfer: transferred.verdict }, { registrations: 1, transfer: "promoted" },
  "root bytes already stored as a control still need a family-root ledger entry");
  assert.deepEqual(state.packages[control.sha256], first, "family use must not rewrite first provenance");
});

test("the same bytes used as roots by two families get one registration in each family", () => {
  const state = createState({ lab: "A", seed: "shared-root" });
  for (const id of ["first", "second"]) {
    const family = simpleFamily(id, "common-root", `${id}-win`);
    step(state, family, `${id}-loss`);
    step(state, family, `${id}-win`);
  }
  assert.deepEqual(["first", "second"].map((id) => rootEvents(state, id, "common-root").length), [1, 1]);
});

test("recording an imported ancestor as a family root preserves ancestry when a child is exported", () => {
  const family = { id: "sender-family", root: () => "root", instances: panel,
    score: (pkg, instance) => ({ success: pkg.source === "descendant" ||
      (pkg.source === "ancestor" && instance.endsWith("/0")) }) };
  const sender = createState({ lab: "A", seed: "source" });
  step(sender, family, "ancestor");
  step(sender, family, "descendant");
  const middle = createState({ lab: "B", seed: "receiver" });
  assert.equal(adopt(middle, { bundle: exportBundle(sender, family.id), family, criterion, heldout: 2 }).verdict, "promoted");
  const original = middle.packages[sha256("ancestor")];
  assert.equal(original.source, undefined);
  const other = simpleFamily("other-family", "ancestor", "local-child");
  assert.equal(step(middle, other, "local-child").verdicts[0].verdict, "promoted");
  const bundle = exportBundle(middle, other.id);
  assert.deepEqual(bundle.lineage.map((p) => p.sha256), ["local-child", "ancestor", "root"].map(sha256));
  const target = createState({ lab: "C", seed: "fresh" });
  const transferred = adopt(target, { bundle, family: other, criterion, heldout: 2 });
  const after = middle.packages[sha256("ancestor")];
  assert.deepEqual({ registrations: rootEvents(middle, other.id, "ancestor").length,
    source: after.source, parent: after.parent, origin: after.origin, transfer: transferred.verdict },
  { registrations: 1, source: "ancestor", parent: original.parent, origin: original.origin, transfer: "promoted" },
  "a family-root event must not replace the imported ancestor's ledger parent with null");
});
