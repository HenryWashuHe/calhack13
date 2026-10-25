// Review guard (Henry's Claude): drives run.mjs's own arm wiring with a stub provider and checks what
// each arm's requests contain. No key, no network. Independent requests must carry no scores, archive or
// reflection; the feedback arm must; every request must fit the frozen 30,000-byte body limit.
import test from "node:test";
import assert from "node:assert/strict";
import { createState, runGeneration } from "../../scripts/iteration/engine.mjs";
import { jobshopFamily, featureDoc } from "../../scripts/iteration/families/jobshop.mjs";
import { claudeMutator } from "../../scripts/iteration/mutators/claude.mjs";
import { cascade } from "../../scripts/iteration/cascade.mjs";
import { independentSampler } from "./run.mjs";

const RULES = [
  "(op) => -op.duration", "(op) => op.remainingWork", "(op) => -op.duration + 0.3 * op.remainingWork",
  "(op) => op.remainingOps * 10 - op.duration", "(op) => op.duration", "(op) => -op.readyAt",
  "(op) => op.remainingWork / (1 + op.duration)", "(op) => -op.duration - 0.1 * op.readyAt",
];
const reply = (k, g) => ({ model: "claude-sonnet-5-5", stop_reason: "end_turn", usage: { input_tokens: 1, output_tokens: 1 },
  text: RULES.slice(0, k).map((r, i) => `=== CANDIDATE ${i + 1} ===\n\`\`\`js\n${r.replace("=>", `=> ${g * 0.01 + i * 0.001} * 0 +`)}\n\`\`\``).join("\n") });

function drive(arm, seed, rounds = 5) {
  const family = jobshopFamily({ id: "jobshop-10x5", seed });
  const requests = [];
  const makeInner = () => claudeMutator({ task: "TASK\nThis is independent repetition 1.", featureDoc, model: "claude-sonnet-5-5", effort: "medium",
    call: (req) => { requests.push(req); return reply(Number(/Write (\d+) distinct/.exec(req.user)[1]), requests.length); } });
  const learned = makeInner();
  const independent = independentSampler(makeInner, family.root());
  const mutator = cascade(arm === "feedback" ? learned : independent, { family, screenSeed: `${seed}:screen`, m: 3, factor: 2 });
  const state = createState({ lab: "judged_demo", seed });
  for (let round = 1; round <= rounds; round++) {
    const result = runGeneration(state, { family, mutator, k: 4, controls: [], criterion: { rule: "success-count-margin", min_margin: 1 }, heldout: 20 });
    if (result.plan && arm === "feedback") {
      const parent = state.packages[result.plan.arms[0].package];
      const parentScores = state.ledger.records.find((x) => x.type === "evaluation" && x.plan === result.plan.sha256 && x.slot === "incumbent")?.scores;
      mutator.observe({ generation: round, parent, parentScores, verdicts: result.verdicts, of: 20 });
    }
  }
  return requests.map((r) => ({ ...r, bytes: Buffer.byteLength(JSON.stringify({ model: r.model, max_tokens: r.max_tokens, system: r.system, messages: [{ role: "user", content: r.user }], output_config: { effort: r.effort } })) }));
}

test("independent requests carry no feedback; feedback requests do; both fit the body limit", () => {
  const ind = drive("independent", "review-iso");
  const fb = drive("feedback", "review-iso");
  assert.equal(ind.length, 5);
  assert.equal(fb.length, 5);
  for (const r of ind) {
    assert.match(r.user, /starting point, not yet scored/);
    assert.doesNotMatch(r.user, /Last generation|\| # \| success/);
    assert.equal((r.user.match(/### Program /g) || []).length, 1);
  }
  assert.ok(fb.slice(1).some((r) => /\| # \| success/.test(r.user)), "feedback arm never saw a score table");
  for (const r of [...ind, ...fb]) assert.ok(r.bytes <= 30_000, `request of ${r.bytes} bytes exceeds the frozen limit`);
  console.log(JSON.stringify({ independent_bytes: ind.map((r) => r.bytes), feedback_bytes: fb.map((r) => r.bytes) }));
});
