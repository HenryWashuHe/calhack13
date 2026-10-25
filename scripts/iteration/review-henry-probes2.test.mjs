import test from "node:test";
import assert from "node:assert/strict";
const E = new URL(".", import.meta.url).pathname.replace(/\/$/, "");
const { createState, runGeneration, adopt, sha256, canon } = await import(E + "/engine.mjs");
const criterion = { rule: "success-count-margin", min_margin: 1 };
const fam = { id: "f", root: () => "root", instances: (seed, n) => Array.from({ length: n }, (_, i) => `f/${seed.slice(0, 12)}/${i}`), score: (p) => ({ success: p.source === "x" }) };

test("P9 fabricated ledger (no plan, no evaluation, no verdict) passes provenance", () => {
  const GENESIS = sha256("genesis-iteration/1");
  const recs = []; let head = GENESIS;
  const add = (r) => { const body = { ...r, lab: "liar", index: recs.length, prev: head }; head = sha256(canon(body)); recs.push({ ...body, sha256: head }); };
  add({ type: "candidates_proposed", generation: 1, family: "f", parent: null, candidates: [sha256("x")], dropped: [] });
  add({ type: "promotion", generation: 1, family: "f", package: sha256("x"), parent: null, success_count: 99, incumbent_count: 0, of: 99, plan: "0".repeat(64) });
  const bundle = { package: { sha256: sha256("x"), source: "x", parent: null, origin: "liar", generation: 1 }, family: "f", lineage: [{ sha256: sha256("x"), origin: "liar", parent: null }], ledger: { records: recs, head }, ledger_sha256: head, exported_by: "liar" };
  const s = createState({ lab: "b", seed: 1 });
  const out = adopt(s, { bundle, family: fam, criterion, heldout: 3 });
  console.log("fabricated bundle adopt verdict:", out.verdict);
  assert.notEqual(out.verdict, "promoted", "a promotion with no frozen plan/evaluations/verdict behind it should not count as provenance");
});

test("P10 lab-scoped seeds collide across labs via ':' concatenation", () => {
  const a = createState({ lab: "org", seed: "x:1" }), b = createState({ lab: "org:x", seed: "1" });
  const m = { propose: () => [{ source: "x" }] };
  const pa = runGeneration(a, { family: fam, mutator: m, k: 1, criterion, heldout: 3 }).plan;
  const pb = runGeneration(b, { family: fam, mutator: m, k: 1, criterion, heldout: 3 }).plan;
  console.log("same panel across labs:", pa.instance_seed === pb.instance_seed);
  assert.notEqual(pa.instance_seed, pb.instance_seed);
});
