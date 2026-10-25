// Independent PR144 regressions. Inert strings and finite in-memory callbacks only.
import test from "node:test";
import assert from "node:assert/strict";
import {
  adopt, createState, exportBundle, runGeneration, sha256, verifyLedger,
} from "./engine.mjs";

const criterion = () => ({ rule: "success-count-margin", min_margin: 1 });
const family = {
  id: "review-provenance",
  root: () => "root",
  instances: (_seed, n) => Array.from({ length: n }, (_, i) => String(i)),
  score: (pkg) => ({
    success: pkg.source !== "root", progress: 1, cost: 1, detail: {},
  }),
};
function winner(lab) {
  const state = createState({ lab, seed: lab });
  const policy = criterion();
  runGeneration(state, {
    family, criterion: policy, k: 1, heldout: 1,
    mutator: { propose: () => [{ source: "winner" }] },
  });
  return { state, policy };
}

test("ledger verification binds the declared terminal head", () => {
  const { state } = winner("A");
  const ledger = structuredClone(state.ledger);
  ledger.head = "0".repeat(64);
  assert.equal(verifyLedger(ledger).ok, false);
});

test("adoption rejects a package and lineage absent from its retained sender evidence", () => {
  const { state } = winner("A");
  const bundle = structuredClone(exportBundle(state, family.id));
  bundle.package = {
    ...bundle.package,
    source: "replacement never recorded by A",
    sha256: sha256("replacement never recorded by A"),
    origin: "unrelated-origin",
  };
  bundle.lineage = [{ sha256: "f".repeat(64), origin: "invented", generation: 99 }];
  assert.equal(verifyLedger(bundle.ledger).ok, true);
  assert.equal(bundle.ledger.records.some((r) => r.package === bundle.package.sha256), false);
  const receiver = createState({ lab: "B", seed: "B" });
  const result = adopt(receiver, {
    bundle, family, criterion: criterion(), heldout: 1,
  });
  assert.equal(result.verdict, "rejected");
  assert.equal(receiver.frontier[family.id], undefined);
});

test("rejected duplicate adoption preserves the receiver's first package provenance", () => {
  const sender = winner("A").state;
  const receiver = winner("C").state;
  const original = structuredClone(receiver.packages[sha256("winner")]);
  const result = adopt(receiver, {
    bundle: exportBundle(sender, family.id),
    family, criterion: criterion(), heldout: 1,
  });
  assert.equal(result.verdict, "rejected");
  assert.deepEqual(receiver.packages[sha256("winner")], original);
  assert.equal(exportBundle(receiver, family.id).lineage[0].origin, "C");
});

test("changing a reused policy cannot mutate an already frozen plan or ledger", () => {
  const { state, policy } = winner("A");
  const frozen = structuredClone(state.ledger);
  policy.min_margin = 2;
  assert.deepEqual(state.ledger, frozen);
  assert.equal(verifyLedger(state.ledger).ok, true);
});
