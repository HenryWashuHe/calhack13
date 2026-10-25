// Probes for the evidence-shape emitter: one acceptance plan per (candidate, control) pairing with exactly one
// candidate and one control slot, trial-qualified identities, adapter-supplied package references kept apart
// from raw source hashes, and per-pairing verdicts incl. insufficient. Pure functions; nothing written.
import test from "node:test";
import assert from "node:assert/strict";
import { createState, runGeneration, run, sha256 } from "./engine.mjs";
import { toTrialPlans, toCriterion, toVerdicts, isExactRef } from "./emit.mjs";
import { syntheticFamily, syntheticMutator, syntheticControls } from "./synthetic-family.mjs";

const criterion = { rule: "success-count-margin", min_margin: 1 };
const ref = (id, schema = "lab-ref/1", domain = "genesis-lab", hash = "a".repeat(64)) => ({ domain, schema, id, version: "1", sha256: hash });
// The adapter supplies every identity. The method reference is the real package envelope hash, which is
// deliberately NOT the engine's raw source hash.
const envelope = (pkg) => sha256("envelope:" + pkg.sha256);
const refs = {
  criterion: ref("success-count-margin", "lab-criterion/1"), public_task: ref("synthetic-line", "lab-task/1"), evaluator: ref("synthetic-eval", "lab-evaluator/1"),
  environment: ref("node", "lab-environment/1"), generator: ref("synthetic-generator", "lab-generator/1"), instance_set: ref("synthetic-set", "lab-instance-set/1"),
  design_schema: ref("design", "lab-design/1"), metric_schema: ref("metrics", "lab-metrics/1"), scoring_rule: ref("rule", "lab-scoring-rule/1"),
  model_settings: ref("none", "lab-model-settings/1"), development_environment: ref("node", "lab-environment/1"),
  instance: (id) => ref(id, "lab-instance/1"),
  method: (pkg) => ({ domain: "genesis", schema: "development-method-source/1", id: "synthetic-method", version: "0.1.0", sha256: envelope(pkg) }),
  dependencies: () => [ref("python-runtime", "lab-runtime/1", "genesis-lab", "b".repeat(64))],
};
const grants = { development: { model_calls: 0, input_tokens: 0, output_tokens: 0, wall_time_ms: 60000, simulation_steps: 72000 }, scoring: { model_calls: 0, input_tokens: 0, output_tokens: 0, wall_time_ms: 600000, simulation_steps: 14400 } };

function generation(k = 2) {
  const family = syntheticFamily({ id: "synthetic-line", seed: 5, target: 60 });
  const state = createState({ lab: "org_example_b", seed: 5 });
  const controls = syntheticControls(family);
  return { state, family, controls, out: runGeneration(state, { family, mutator: syntheticMutator(), k, controls, criterion, heldout: 3 }) };
}

test("one acceptance plan per (candidate, control-arm) pairing, each with exactly one candidate and one control slot", () => {
  const { out, state, controls } = generation(2);
  const { plans, sources } = toTrialPlans(out.plan, { refs, grants, org_id: "org_example_b", id: "synthetic", packages: state.packages });
  const candidates = out.plan.arms.filter((a) => a.slot.startsWith("candidate")).length;
  assert.equal(plans.length, candidates * (1 + controls.length));
  for (const p of plans) {
    assert.deepEqual(Object.keys(p), ["id", "version", "criterion", "development_instances", "scoring_instances", "context", "model_settings", "development_environment", "development_grant", "scoring_grant", "slots"]);
    assert.equal(p.slots.length, 2);
    assert.deepEqual(p.slots.map((s) => s.purpose), [{ kind: "acceptance", role: "candidate" }, { kind: "acceptance", role: "control" }]);
    assert.deepEqual(p.slots.map((s) => s.slot_id), ["candidate", "control"]);
    for (const s of p.slots) {
      assert.deepEqual(Object.keys(s), ["slot_id", "scoring_request_id", "org_id", "run_id", "attempt_id", "method", "dependencies", "purpose"]);
      assert.ok(isExactRef(s.method)); assert.ok(s.dependencies.every(isExactRef));
    }
    assert.equal(p.development_instances.length, 3);
    assert.ok(sources[p.id].candidate.sha256 && sources[p.id].control.sha256, "raw source identities travel beside the plan, not inside the method ref");
    assert.equal(sources[p.id].candidate.schema, "iteration-source/1");
  }
});

test("the method reference is the adapter's package identity, not the raw source hash", () => {
  const { out, state } = generation(1);
  const { plans, sources } = toTrialPlans(out.plan, { refs, grants, org_id: "x", id: "p", packages: state.packages });
  const p = plans[0];
  assert.notEqual(p.slots[0].method.sha256, sources[p.id].candidate.sha256);
  assert.equal(p.slots[0].method.sha256, envelope(state.packages[sources[p.id].candidate.sha256]));
});

test("run, attempt and scoring identities are trial-qualified: distinct across generations and pairings", () => {
  const family = syntheticFamily({ id: "synthetic-line", seed: 5, target: 60 });
  const state = createState({ lab: "org_example_b", seed: 5 });
  const controls = syntheticControls(family);
  const g1 = runGeneration(state, { family, mutator: syntheticMutator(), k: 2, controls, criterion, heldout: 3 });
  const g2 = runGeneration(state, { family, mutator: syntheticMutator(), k: 2, controls, criterion, heldout: 3 });
  const ids = (out) => toTrialPlans(out.plan, { refs, grants, org_id: "x", id: "p", packages: state.packages }).plans.flatMap((p) => p.slots.flatMap((s) => [s.run_id, s.attempt_id, s.scoring_request_id]));
  const a = ids(g1), b = ids(g2);
  assert.equal(new Set(a).size, a.length, "no identity is reused within a generation");
  assert.ok(a.every((x) => !b.includes(x)), "no identity is reused across generations");
});

test("the emitter refuses to invent identities or grants", () => {
  const { out, state } = generation(1);
  assert.throws(() => toTrialPlans(out.plan, { refs: { ...refs, evaluator: undefined }, grants, org_id: "x", id: "p", packages: state.packages }), /evaluator/);
  assert.throws(() => toTrialPlans(out.plan, { refs: { ...refs, method: () => ({ id: "no-hash" }) }, grants, org_id: "x", id: "p", packages: state.packages }), /method/);
  assert.throws(() => toTrialPlans(out.plan, { refs, grants: {}, org_id: "x", id: "p" }), /grant/);
  assert.throws(() => toTrialPlans(out.plan, { refs, grants, org_id: "", id: "p" }), /org_id/);
});

test("criterion.json matches the retained shape; verdicts are per pairing with candidate, control, of, and insufficient when unknown", () => {
  const { out } = generation(2);
  const crit = toCriterion(out.plan);
  assert.deepEqual(Object.keys(crit), ["instances", "min_margin", "rule"]);
  for (const v of out.verdicts) {
    const docs = toVerdicts(v, out.plan);
    assert.equal(docs.length, v.margins.length);
    for (const d of docs) {
      assert.deepEqual(Object.keys(d.document), ["candidate", "control", "of", "reason", "verdict"]);
      assert.ok(["accepted", "rejected", "insufficient"].includes(d.document.verdict));
      const m = v.margins.find((x) => x.against === d.against);
      assert.equal(d.document.verdict, m.margin >= criterion.min_margin ? "accepted" : "rejected", "acceptance is per pairing, independent of the one-frontier selection");
    }
  }
  const unknown = { ...out.verdicts[0], unknown: true, margins: out.verdicts[0].margins.map((m) => ({ ...m, known: false })) };
  assert.ok(toVerdicts(unknown, out.plan).every((d) => d.document.verdict === "insufficient"));
});

test("a candidate that clears the margin but loses the frontier tie-break still receives an accepted pairing document", () => {
  // Scripted: two candidates both beat the root on the same counts; the engine promotes only one.
  const fam = { id: "scripted", root: () => "root", instances: (seed, n) => Array.from({ length: n }, (_, i) => `s/${i}`), score: (pkg) => ({ success: pkg.source !== "root", progress: 0, cost: 1, detail: {} }) };
  const state = createState({ lab: "x", seed: 1 });
  const out = runGeneration(state, { family: fam, mutator: { propose: () => [{ source: "c1" }, { source: "c2" }] }, k: 2, criterion, heldout: 2 });
  assert.deepEqual(out.verdicts.map((v) => v.verdict).sort(), ["promoted", "rejected"]);
  for (const v of out.verdicts) assert.ok(toVerdicts(v, out.plan).every((d) => d.document.verdict === "accepted"), "both cleared the margin against the incumbent");
  void run;
});
