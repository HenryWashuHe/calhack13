// Independent synthetic-only guards: finite callbacks and inert source strings.
// No generated source, provider, process, lab runtime or evaluator is invoked.
import test from "node:test";
import assert from "node:assert/strict";
import { adopt, createState, exportBundle, runGeneration, sha256 } from "./engine.mjs";

const criterion = { rule: "success-count-margin", min_margin: 1 };
const fixed = (source) => ({ propose: () => [{ source }] });
const panel = (seed, n) => Array.from({ length: n }, (_, i) => `${seed}/${i}`);

test("resumption retains earlier invalid observations and does not repeat completed instances", () => {
  const state = createState({ lab: "review", seed: "partial" });
  let failed = false;
  const seen = [];
  const family = {
    id: "partial", root: () => "root", instances: panel,
    score(pkg, instance) {
      seen.push(`${pkg.source}:${instance}`);
      if (pkg.source === "root" && instance.endsWith("/0")) return { success: "unknown" };
      if (pkg.source === "root" && instance.endsWith("/2") && !failed) {
        failed = true;
        throw new Error("one interrupted evaluation");
      }
      return { success: pkg.source === "candidate" };
    },
  };
  const request = { family, mutator: fixed("candidate"), criterion, k: 1, heldout: 3 };
  assert.throws(() => runGeneration(state, request), /interrupted/);
  const result = runGeneration(state, request);
  const repeated = seen.filter((key) => key.startsWith("root:") && key.endsWith("/1"));
  assert.deepEqual(
    { verdict: result.verdicts[0].verdict, repeatsOfCompletedInstance: repeated.length },
    { verdict: "insufficient", repeatsOfCompletedInstance: 1 },
    "an invalid observation cannot disappear into a resumed promotion",
  );
});

test("throwing a primitive or frozen error cannot erase already completed partial scores", () => {
  const completedCounts = [];
  for (const thrown of ["interrupted", Object.freeze(new Error("interrupted"))]) {
    const state = createState({ lab: "review", seed: "error-shape" });
    let failed = false;
    const seen = [];
    const family = {
      id: "throw-shape", root: () => "root", instances: panel,
      score(pkg, instance) {
        seen.push(`${pkg.source}:${instance}`);
        if (pkg.source === "root" && instance.endsWith("/1") && !failed) {
          failed = true;
          throw thrown;
        }
        return { success: pkg.source === "candidate" };
      },
    };
    const request = { family, mutator: fixed("candidate"), criterion, k: 1, heldout: 2 };
    assert.throws(() => runGeneration(state, request));
    runGeneration(state, request);
    completedCounts.push(seen.filter((key) => key.startsWith("root:") && key.endsWith("/0")).length);
  }
  assert.deepEqual(completedCounts, [1, 1], "partial results must be retained without mutating the thrown value");
});

test("an imported ancestor can later be evaluated using its verified source without rewriting provenance", () => {
  const senderFamily = {
    id: "transfer", root: () => "root", instances: panel,
    score: (pkg, instance) => ({ success: pkg.source === "second" ||
      (pkg.source === "first" && instance.endsWith("/0")) }),
  };
  const sender = createState({ lab: "A", seed: "fixed" });
  runGeneration(sender, { family: senderFamily, mutator: fixed("first"), criterion, k: 1, heldout: 2 });
  const first = exportBundle(sender, senderFamily.id);
  runGeneration(sender, { family: senderFamily, mutator: fixed("second"), criterion, k: 1, heldout: 2 });
  const second = exportBundle(sender, senderFamily.id);
  const receiverFamily = {
    ...senderFamily,
    score: (pkg, instance) => ({ success: pkg.source === "first" ||
      (pkg.source === "second" && instance.endsWith("/0")) }),
  };
  const receiver = createState({ lab: "B", seed: "fixed" });
  assert.equal(adopt(receiver, { bundle: second, family: receiverFamily, criterion, heldout: 2 }).verdict, "promoted");
  const prior = receiver.packages[sha256("first")];
  assert.equal(prior.origin, "A");
  assert.equal(adopt(receiver, { bundle: first, family: receiverFamily, criterion, heldout: 2 }).verdict,
    "promoted", "metadata-only knowledge must not discard the validated incoming source");
  assert.equal(receiver.frontier.transfer.package.source, "first");
  assert.equal(receiver.packages[sha256("first")].origin, prior.origin);
  assert.equal(receiver.packages[sha256("first")].parent, prior.parent);
});
