// Independent synthetic-only boundary: inert source labels and finite callbacks.
// No generated source, evaluator runtime, subprocess, network or provider is used.
import test from "node:test";
import assert from "node:assert/strict";
import { adopt, createState, exportBundle, runGeneration, sha256 } from "./engine.mjs";

const criterion = { rule: "success-count-margin", min_margin: 1 };
const panel = (seed, n) => Array.from({ length: n }, (_, i) => `${seed}/${i}`);
const propose = (source) => ({ propose: () => [{ source }] });

test("a metadata-only imported ancestor used as another family root receives its verified bytes", () => {
  const family = {
    id: "sender-family", root: () => "root", instances: panel,
    score: (pkg, instance) => ({ success: pkg.source === "descendant" ||
      (pkg.source === "ancestor" && instance.endsWith("/0")) }),
  };
  const sender = createState({ lab: "A", seed: "source" });
  for (const source of ["ancestor", "descendant"]) {
    runGeneration(sender, { family, mutator: propose(source), criterion, k: 1, heldout: 2 });
  }
  const receiver = createState({ lab: "B", seed: "receiver" });
  assert.equal(adopt(receiver, { bundle: exportBundle(sender, family.id), family, criterion, heldout: 2 }).verdict, "promoted");
  const before = receiver.packages[sha256("ancestor")];
  assert.equal(before.source, undefined, "the ancestor initially carries metadata only");
  const seen = [];
  const other = {
    id: "other-family", root: () => "ancestor", instances: panel,
    score(pkg, instance) {
      seen.push(pkg.source);
      return { success: pkg.source === "ancestor" ||
        (pkg.source === "local-candidate" && instance.endsWith("/0")) };
    },
  };
  const out = runGeneration(receiver, { family: other, mutator: propose("local-candidate"), criterion, k: 1, heldout: 2 });
  assert.deepEqual({ verdict: out.verdicts[0].verdict, rootSources: seen.slice(0, 2) },
    { verdict: "rejected", rootSources: ["ancestor", "ancestor"] },
    "the 1/2 candidate must not beat a 2/2 root whose bytes were discarded");
  const after = receiver.packages[sha256("ancestor")];
  assert.equal(after.source, "ancestor");
  assert.equal(after.origin, before.origin);
  assert.equal(after.parent, before.parent);
});
