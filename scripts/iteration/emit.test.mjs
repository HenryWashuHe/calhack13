// Probes for the evidence-shape emitter: an engine plan and verdict rendered in the shapes already used
// under docs/invention-lab/evidence/*/{plan,criterion,verdict}.json. Pure functions; no files written here.
import test from "node:test";
import assert from "node:assert/strict";
import { createState, runGeneration } from "./engine.mjs";
import { toTrialPlan, toCriterion, toVerdict, isExactRef } from "./emit.mjs";
import { syntheticFamily, syntheticMutator, syntheticControls } from "./synthetic-family.mjs";

const criterion = { rule: "success-count-margin", min_margin: 1 };
const ref = (id, schema = "lab-ref/1", domain = "genesis-lab") => ({ domain, schema, id, version: "1", sha256: "a".repeat(64) });
// The adapter supplies every ExactContentRef; the emitter never invents one.
const refs = {
  criterion: ref("success-count-margin", "lab-criterion/1"), public_task: ref("synthetic-line", "lab-task/1"), evaluator: ref("synthetic-eval", "lab-evaluator/1"),
  environment: ref("node", "lab-environment/1"), generator: ref("synthetic-generator", "lab-generator/1"), instance_set: ref("synthetic-set", "lab-instance-set/1"),
  design_schema: ref("design", "lab-design/1"), metric_schema: ref("metrics", "lab-metrics/1"), scoring_rule: ref("rule", "lab-scoring-rule/1"),
  model_settings: ref("none", "lab-model-settings/1"), development_environment: ref("node", "lab-environment/1"),
  instance: (id) => ref(id, "lab-instance/1"), method: (pkg) => ({ domain: "genesis", schema: "development-method-source/1", id: pkg.note || "candidate", version: "0.1.0", sha256: pkg.sha256 }),
};
const grants = { development: { model_calls: 0, input_tokens: 0, output_tokens: 0, wall_time_ms: 60000, simulation_steps: 72000 }, scoring: { model_calls: 0, input_tokens: 0, output_tokens: 0, wall_time_ms: 600000, simulation_steps: 14400 } };

function generation() {
  const family = syntheticFamily({ id: "synthetic-line", seed: 5, target: 60 });
  const state = createState({ lab: "org_example_b", seed: 5 });
  return { state, family, out: runGeneration(state, { family, mutator: syntheticMutator(), k: 2, controls: syntheticControls(family), criterion, heldout: 3 }) };
}

test("a frozen engine plan renders as a development-trial-plan/1 with one slot per arm, in plan order", () => {
  const { out } = generation();
  const plan = toTrialPlan(out.plan, { refs, grants, org_id: "org_example_b", id: "synthetic-gen-1" });
  assert.deepEqual(Object.keys(plan), ["id", "version", "criterion", "development_instances", "scoring_instances", "context", "model_settings", "development_environment", "development_grant", "scoring_grant", "slots"]);
  assert.equal(plan.version, "1");
  assert.ok(isExactRef(plan.criterion));
  assert.equal(plan.development_instances.length, 3);
  assert.ok(plan.development_instances.every(isExactRef));
  assert.deepEqual(plan.scoring_instances, { kind: "development" });
  assert.deepEqual(Object.keys(plan.context), ["public_task", "evaluator", "environment", "generator", "instance_set", "design_schema", "metric_schema", "scoring_rule"]);
  assert.equal(plan.slots.length, out.plan.arms.length);
  assert.deepEqual(plan.slots.map((s) => s.slot_id), out.plan.arms.map((a) => a.slot));
  for (const s of plan.slots) {
    assert.deepEqual(Object.keys(s), ["slot_id", "scoring_request_id", "org_id", "run_id", "attempt_id", "method", "dependencies", "purpose"]);
    assert.ok(isExactRef(s.method));
    assert.equal(s.method.sha256, out.plan.arms.find((a) => a.slot === s.slot_id).package);
    assert.deepEqual(s.dependencies, []);
  }
  assert.deepEqual(plan.slots[0].purpose, { kind: "acceptance", role: "control" }, "the incumbent is a control arm in trial terms");
  assert.ok(plan.slots.some((s) => s.purpose.role === "candidate"));
});

test("the emitter refuses to invent identities: a missing or malformed ref fails loudly", () => {
  const { out } = generation();
  assert.throws(() => toTrialPlan(out.plan, { refs: { ...refs, evaluator: undefined }, grants, org_id: "x", id: "p" }), /evaluator/);
  assert.throws(() => toTrialPlan(out.plan, { refs: { ...refs, public_task: { id: "no-hash" } }, grants, org_id: "x", id: "p" }), /public_task/);
  assert.throws(() => toTrialPlan(out.plan, { refs, grants: {}, org_id: "x", id: "p" }), /grant/);
});

test("criterion.json and verdict.json match the retained shapes and vocabulary", () => {
  const { out } = generation();
  const crit = toCriterion(out.plan);
  assert.deepEqual(Object.keys(crit), ["instances", "min_margin", "rule"]);
  assert.equal(crit.rule, "success-count-margin");
  assert.deepEqual(crit.instances, out.plan.instances);
  for (const v of out.verdicts) {
    const j = toVerdict(v, out.plan);
    assert.deepEqual(Object.keys(j), ["reason", "verdict"]);
    assert.ok(["accepted", "rejected"].includes(j.verdict), "the retained vocabulary is accepted/rejected, not promoted");
    assert.equal(j.verdict, v.verdict === "promoted" ? "accepted" : "rejected");
    assert.match(j.reason, /candidate \d+\/\d+ /);
    assert.match(j.reason, /by 1/);
  }
});
