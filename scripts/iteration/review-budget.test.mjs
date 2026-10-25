// Independent, in-memory checks only; no generated code, provider or runtime adapter.
import test from "node:test";
import assert from "node:assert/strict";
import { createState, run, runGeneration } from "./engine.mjs";

const criterion = { rule: "success-count-margin", min_margin: 1 };
const mutator = { propose: () => [{ source: "candidate" }] };
const family = {
  id: "pure-fixture", root: () => "root",
  instances: (_seed, n) => Array.from({ length: n }, (_, i) => `instance-${i}`),
  score: () => ({ success: false, progress: 0, cost: 1, detail: {} }),
};

test("a panel larger than its declared allowance is refused before any scoring", () => {
  const state = createState({ lab: "a", seed: "test" });
  let scores = 0;
  const oversized = {
    ...family, instances: () => Array.from({ length: 10 }, (_, i) => `instance-${i}`),
    score: () => { scores++; return family.score(); },
  };
  try {
    run(state, { family: oversized, mutator, k: 1, criterion, heldout: 1,
      generations: 1, budget: { evaluations: 2, calls: 1 } });
  } catch { /* A fail-closed adapter error is valid. */ }
  assert.equal(scores, 0, "the adapter returned ten instances against one declared per arm");
  assert.ok(state.evaluations <= 2, "the declared generation ceiling is never overrun");
});

test("a truthy non-boolean score cannot promote a candidate", () => {
  const state = createState({ lab: "a", seed: "test" });
  const malformed = {
    ...family,
    score: (pkg) => ({ ...family.score(), success: pkg.source === "candidate" ? "false" : false }),
  };
  try { runGeneration(state, { family: malformed, mutator, k: 1, criterion, heldout: 1 }); }
  catch { /* Invalid score evidence may be refused. */ }
  assert.equal(state.frontier[family.id], undefined);
  assert.ok(!state.ledger.records.some((r) => r.type === "promotion"));
});

test("a failed scoring attempt still consumes its evaluation count and has a retained outcome", () => {
  const state = createState({ lab: "a", seed: "test" });
  let scores = 0;
  const interrupted = {
    ...family, score: () => {
      scores++;
      if (scores === 2) throw new Error("fixture adapter interruption");
      return family.score();
    },
  };
  try {
    run(state, { family: interrupted, mutator, k: 1, criterion, heldout: 2,
      generations: 1, budget: { evaluations: 4, calls: 1 } });
  } catch { /* Propagating the error must not erase already-attempted work. */ }
  assert.equal(scores, 2);
  assert.equal(state.evaluations, 2);
  assert.ok(state.ledger.records.some((r) =>
    JSON.stringify(r).includes("failed") || JSON.stringify(r).includes("unknown")),
  "the interrupted outcome remains explicit rather than silently disappearing");
});
