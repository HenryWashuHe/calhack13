// Run: node --test scripts/iteration/cascade.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import { cascade } from "./cascade.mjs";
import { createState, runGeneration, sha256 } from "./engine.mjs";

const criterion = { rule: "success-count-margin", min_margin: 1 };
const family = { id: "fam", root: () => "0", instances: (seed, n) => Array.from({ length: n }, (_, i) => `fam/${String(seed).slice(0, 8)}/${i}`),
  score: (pkg) => ({ success: Number(pkg.source) >= 5, progress: Math.min(1, Number(pkg.source) / 10), cost: 1 }) };
const inner = (sources) => { const seen = []; return { seen, propose: ({ k }) => { seen.push(k); return sources.map((source) => ({ source })); } }; };

test("the cascade asks for factor×k, screens on its own panel and keeps the top k", () => {
  const base = inner(["1", "9", "3", "7"]);
  const c = cascade(base, { family, screenSeed: "s", m: 2, factor: 2 });
  const out = c.propose({ parent: { source: "0" }, feedback: null, k: 2, generation: 1 });
  assert.deepEqual(base.seen, [4]);
  assert.deepEqual(out.map((o) => o.source), ["9", "7"]);
  assert.equal(c.counts.evaluations, 8);
  assert.match(out[0].note, /screened 2\/2/);
});

test("screening instances never coincide with the engine's sealed panel for the same generation", () => {
  const seenByEngine = new Set();
  const spy = { ...family, score: (pkg, instance) => { seenByEngine.add(instance); return family.score(pkg); } };
  const c = cascade(inner(["1", "9", "3"]), { family: spy, screenSeed: "s", m: 2 });
  const state = createState({ lab: "A", seed: "x" });
  const out = runGeneration(state, { family: spy, mutator: c, criterion, k: 1, heldout: 2 });
  const planPanel = new Set(out.plan.instances);
  const screening = [...seenByEngine].filter((i) => !planPanel.has(i));
  assert.equal(screening.length, 2, "screening used its own two instances");
  assert.ok(screening.every((i) => !planPanel.has(i)));
});

test("when the inner mutator returns k or fewer, nothing is screened or spent", () => {
  const c = cascade(inner(["4"]), { family, screenSeed: "s", m: 3 });
  assert.equal(c.propose({ parent: { source: "0" }, k: 2, generation: 1 }).length, 1);
  assert.equal(c.counts.evaluations, 0);
});
