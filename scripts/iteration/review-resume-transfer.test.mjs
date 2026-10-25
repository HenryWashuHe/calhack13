// Independent retained-protocol regressions: finite in-memory callbacks and inert source strings only.
import test from "node:test";
import assert from "node:assert/strict";
import { adopt, canon, confirm, createState, exportBundle, run, runGeneration, sha256, verifyLedger } from "./engine.mjs";
import { toTrialPlans } from "./emit.mjs";

const criterion = { rule: "success-count-margin", min_margin: 1 };
const fixed = (source) => ({ propose: () => [{ source }] });
const family = {
  id: "review-transfer", root: () => "root",
  instances: (seed, n) => Array.from({ length: n }, (_, i) => `${seed}/${i}`),
  score: (pkg, instance) => ({ success: pkg.source === "second" || (pkg.source === "first" && instance.endsWith("/0")), progress: 0, cost: 1, detail: {} }),
};
function sender() {
  const state = createState({ lab: "A", seed: "fixed" });
  for (const source of ["first", "second"]) runGeneration(state, { family, mutator: fixed(source), k: 1, heldout: 2, criterion });
  assert.equal(state.frontier[family.id].package.source, "second");
  return state;
}

test("a failed final confirmation consumes the frozen endpoint and cannot be retried as a new reading", () => {
  const state = sender(); let calls = 0;
  const failing = { ...family, score: (pkg, instance) => { calls++; if (calls === 2) throw new Error("interrupted final scorer"); return family.score(pkg, instance); } };
  assert.throws(() => confirm(state, { family: failing, heldout: 2, panelSeed: "final" }));
  const spent = state.evaluations;
  assert.equal(state.ledger.records.filter((r) => r.type === "confirmation_declared").length, 1);
  assert.throws(() => confirm(state, { family, heldout: 2, panelSeed: "final" }), /already|used|pending|failed|once/i);
  assert.equal(state.evaluations, spent, "a failed endpoint must not be silently rescored");
});

test("resumption admits the pending frozen panel cost rather than new smaller request parameters", () => {
  const state = createState({ lab: "A", seed: "fixed" }); let calls = 0, fail = true;
  const flaky = { ...family, score: (pkg, instance) => { calls++; if (fail && calls === 2) throw new Error("interrupted arm"); return family.score(pkg, instance); } };
  const budget = { evaluations: 8, calls: 2 };
  assert.throws(() => run(state, { family: flaky, mutator: fixed("second"), k: 1, heldout: 4, criterion, generations: 1, budget }));
  assert.equal(state.evaluations, 2); fail = false;
  run(state, { family: flaky, mutator: fixed("second"), k: 1, heldout: 1, criterion, generations: 1, budget });
  assert.ok(state.evaluations <= budget.evaluations, `spent ${state.evaluations} under ceiling ${budget.evaluations}`);
});

test("an accepted import re-exports all exact ancestry through the sender's root", () => {
  const original = exportBundle(sender(), family.id);
  const receiver = createState({ lab: "B", seed: "fixed" });
  assert.equal(adopt(receiver, { bundle: original, family, criterion, heldout: 2 }).verdict, "promoted");
  const forwarded = exportBundle(receiver, family.id);
  assert.deepEqual(forwarded.lineage.map((p) => p.sha256), original.lineage.map((p) => p.sha256));
});

test("a legitimate adopted package can be forwarded without becoming a forged local proposal", () => {
  const original = exportBundle(sender(), family.id);
  const receiver = createState({ lab: "B", seed: "fixed" });
  assert.equal(adopt(receiver, { bundle: original, family, criterion, heldout: 2 }).verdict, "promoted");
  const next = createState({ lab: "C", seed: "fixed" });
  const result = adopt(next, { bundle: exportBundle(receiver, family.id), family, criterion, heldout: 2 });
  assert.equal(result.verdict, "promoted", result.reason);
});

test("canonical numeric tags cannot alias ordinary score objects", () => {
  const state = createState({ lab: "A", seed: "fixed" });
  const tagged = { ...family, score: (pkg, instance) => ({ ...family.score(pkg, instance), progress: Infinity }) };
  runGeneration(state, { family: tagged, mutator: fixed("second"), k: 1, heldout: 2, criterion });
  const altered = structuredClone(state.ledger);
  altered.records.find((r) => r.type === "evaluation").scores[0].progress = { $n: "Infinity" };
  assert.equal(verifyLedger(altered).ok, false, "a numeric outcome replaced by an object must change its identity");
});

test("distinct valid control labels cannot collapse to the same emitted trial and execution identities", () => {
  const ref = (id) => ({ domain: "review", schema: "ref/1", id, version: "1", sha256: sha256(id) });
  const refs = Object.fromEntries(["criterion", "public_task", "evaluator", "environment", "generator", "instance_set", "design_schema", "metric_schema", "scoring_rule", "model_settings", "development_environment"].map((key) => [key, ref(key)]));
  refs.instance = ref; refs.method = (pkg) => ref(pkg.sha256);
  const grant = { model_calls: 0, input_tokens: 0, output_tokens: 0, wall_time_ms: 1, simulation_steps: 1 };
  const planBody = { family: "review", generation: 1, instances: ["i1"], criterion, arms: [
    { slot: "candidate:0", package: sha256("candidate") },
    { slot: "control:foo_bar", package: sha256("control-one") },
    { slot: "control:foo-bar", package: sha256("control-two") },
  ] };
  const plan = { ...planBody, sha256: sha256(canon(planBody)) };
  const { plans } = toTrialPlans(plan, { refs, grants: { development: grant, scoring: grant }, org_id: "org_a", id: "review" });
  assert.equal(new Set(plans.map((p) => p.id)).size, plans.length, "pairing labels must remain distinct after normalization");
  const runIds = plans.flatMap((p) => p.slots.map((s) => s.run_id));
  assert.equal(new Set(runIds).size, runIds.length);
});
