// Inert probes for the model mutator: prompt contents, id hygiene, parsing, failure handling. No network:
// the transport is injected. Run: node --test scripts/iteration/mutators/claude.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import { claudeMutator, parseCandidates } from "./claude.mjs";
import { createState, runGeneration, sha256 } from "../engine.mjs";

const criterion = { rule: "success-count-margin", min_margin: 1 };
const reply = (sources) => ({ text: sources.map((s, i) => `=== CANDIDATE ${i + 1} ===\n\`\`\`js\n${s}\n\`\`\``).join("\n"), stop_reason: "end_turn", model: "fake", usage: { input_tokens: 10, output_tokens: 5 } });
const family = { id: "fam", root: () => "(op) => -op.duration", instances: (seed, n) => Array.from({ length: n }, (_, i) => `fam/${String(seed).slice(0, 8)}/${i}`),
  score: (pkg, instance) => ({ success: pkg.source.includes("remainingWork") || instance.endsWith("/0"), progress: pkg.source.includes("remainingWork") ? 0.9 : 0.5, cost: 1, detail: { ratio: 1.2 } }) };

test("parseCandidates reads the declared layout and falls back to fenced js blocks", () => {
  assert.deepEqual(parseCandidates(reply(["(op) => 1", "(op) => 2"]).text), ["(op) => 1", "(op) => 2"]);
  assert.deepEqual(parseCandidates("```js\n(op) => 3\n```"), ["(op) => 3"]);
  assert.deepEqual(parseCandidates("no code here"), []);
});

test("the prompt shows scored programs best last with id-free tables, and never an instance id", () => {
  const calls = [];
  const m = claudeMutator({ task: "T", featureDoc: "F", call: (req) => { calls.push(req); return reply(["(op) => op.remainingWork", "(op) => op.duration"]); } });
  const state = createState({ lab: "A", seed: "s" });
  const first = runGeneration(state, { family, mutator: m, criterion, k: 2, heldout: 3 });
  const incumbent = state.ledger.records.find((r) => r.type === "evaluation" && r.plan === first.plan.sha256 && r.slot === "incumbent");
  m.observe({ generation: 1, parent: state.packages[first.plan.arms[0].package], parentScores: incumbent.scores, verdicts: first.verdicts, of: 3 });
  runGeneration(state, { family, mutator: m, criterion, k: 2, heldout: 3 });
  assert.equal(calls.length, 2);
  const second = calls[1].user;
  assert.doesNotMatch(second, /fam\/[0-9a-f]{8}\//, "no instance id reaches the model");
  assert.match(second, /Scored programs \(worst first, best last\)/);
  const order = [...second.matchAll(/### Program \d+: (\d)\/3/g)].map((x) => Number(x[1]));
  assert.ok(order.length >= 2 && order[order.length - 1] === Math.max(...order), "best program comes last");
  assert.match(second, /## Last generation/);
  assert.match(second, /\| 0 \| yes \|/, "per-instance rows are indexed, not named");
  assert.equal(m.usage.calls, 2); assert.equal(m.usage.input_tokens, 20);
});

test("a transport failure or a refusal yields no candidates and is counted, never thrown into the engine", () => {
  const failing = claudeMutator({ task: "T", featureDoc: "F", call: () => { throw new Error("boom"); } });
  const state = createState({ lab: "A", seed: "s" });
  const out = runGeneration(state, { family, mutator: failing, criterion, k: 2, heldout: 2 });
  assert.equal(out.plan, null); assert.equal(failing.usage.failures, 1);
  const refusing = claudeMutator({ task: "T", featureDoc: "F", call: () => ({ text: "", refusal: "general_harms", stop_reason: "refusal" }) });
  assert.deepEqual(refusing.propose({ parent: { source: "x" }, feedback: null, k: 2, generation: 1 }), []);
});

test("candidates are capped at k and carry a note naming the model and generation", () => {
  const m = claudeMutator({ task: "T", featureDoc: "F", model: "m-test", call: () => reply(["(op) => 1", "(op) => 2", "(op) => 3"]) });
  const out = m.propose({ parent: { source: "(op) => 0", sha256: sha256("(op) => 0") }, feedback: null, k: 2, generation: 4 });
  assert.equal(out.length, 2);
  assert.match(out[0].note, /fake, generation 4/);
});
