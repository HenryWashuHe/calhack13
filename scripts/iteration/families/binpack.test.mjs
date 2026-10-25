// Tests for the online bin-packing family: instance determinism, scoring, sandbox containment, controls and
// one end-to-end generation on the engine. Run: node --test scripts/iteration/families/binpack.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import { createState, run, verifyLedger, sha256 } from "../engine.mjs";
import { binpackFamily, binpackControls, problemOf, FEATURES } from "./binpack.mjs";

const family = binpackFamily({ id: "binpack", seed: "s" });
const pkgOf = (source) => ({ sha256: sha256(source), source });
const firstFit = pkgOf(family.root());
const bestFit = pkgOf("(item, bin) => -bin.fits");

test("instances are n distinct prefixed strings, identical for the same seed", () => {
  const a = family.instances("abcdefgh12345", 5), b = family.instances("abcdefgh12345", 5);
  assert.equal(a.length, 5);
  assert.equal(new Set(a).size, 5);
  assert.ok(a.every((i) => i.startsWith("binpack/abcdefgh/")));
  assert.deepEqual(a, b);
  assert.notDeepEqual(a, family.instances("zzzzzzzz", 5));
});

test("instance content: 100 integer sizes in [1, 50] with the volume lower bound", () => {
  const p = problemOf("binpack/x/0", 100, 100);
  assert.equal(p.sizes.length, 100);
  assert.ok(p.sizes.every((s) => Number.isInteger(s) && s >= 1 && s <= 50));
  assert.equal(p.lower_bound, Math.ceil(p.sizes.reduce((a, b) => a + b, 0) / 100));
  assert.deepEqual(problemOf("binpack/x/0", 100, 100), p);
  assert.notDeepEqual(problemOf("binpack/x/1", 100, 100).sizes, p.sizes);
});

test("score is deterministic", () => {
  const [i] = family.instances("det", 1);
  assert.deepEqual(family.score(firstFit, i), family.score(firstFit, i));
  assert.equal(typeof family.score(firstFit, i).success, "boolean");
});

test("lower bound never exceeds the bins used and the bins hold every item (root on 5 instances)", () => {
  for (const i of family.instances("lb", 5)) {
    const { detail } = family.score(firstFit, i);
    const p = problemOf(i, 100, 100);
    assert.ok(detail.lower_bound <= detail.bins, `${i}: ${detail.lower_bound} > ${detail.bins}`);
    assert.ok(detail.bins * 100 >= p.total, `${i}: ${detail.bins} bins cannot hold ${p.total}`);
    assert.ok(detail.ratio >= 1);
  }
});

test("malformed, non-function and throwing sources fail without throwing", () => {
  const [i] = family.instances("bad", 1);
  for (const source of ["(item, bin) =>", "42", '{ "guess": 0 }', "(item, bin) => { throw new Error('boom') }", "(item, bin) => NaN", "(item, bin) => 'high'", "(item, bin) => Infinity", undefined]) {
    const r = family.score({ source }, i);
    assert.equal(r.success, false, String(source));
    assert.equal(r.progress, 0);
    assert.equal(typeof r.detail.error, "string");
  }
  assert.equal(family.score(null, i).success, false);
});

test("an infinite loop at evaluation or inside the rule returns within about 3x the timeout", () => {
  const quick = binpackFamily({ id: "binpack", seed: "s", timeoutMs: 100 });
  const [i] = quick.instances("loop", 1);
  for (const source of ["(() => { while (true) {} })()", "(item, bin) => { while (true) {} }"]) {
    const t0 = performance.now();
    const r = quick.score({ source }, i);
    const elapsed = performance.now() - t0;
    assert.equal(r.success, false);
    assert.match(r.detail.error, /timed out/);
    assert.ok(elapsed < 300, `${source} took ${elapsed}ms`);
  }
});

test("the rule cannot see process, require or timers", () => {
  const [i] = family.instances("leak", 1);
  const r = family.score({ source: '(item, bin) => typeof process === "undefined" ? -bin.index : (() => { throw new Error("leak") })()' }, i);
  assert.equal(typeof r.success, "boolean");
  assert.equal(r.detail.error, undefined);
  for (const name of ["require", "setTimeout", "setInterval"]) {
    assert.equal(family.score({ source: `(item, bin) => { ${name}; return 1 }` }, i).success, false, name);
  }
  for (const source of ["(item, bin) => { import('node:fs'); return 1 }", "(item, bin) => { Promise.reject(new Error('x')); return 1 }", "(item, bin) => { (async () => { throw new Error('y') })(); return 1 }"]) {
    const r = family.score({ source }, i); // a rejected sandbox promise would later crash the host: refused before evaluation
    assert.equal(r.success, false, source);
    assert.match(r.detail.error, /not allowed/);
  }
  assert.equal(family.score({ source: '(item, bin) => typeof globalThis.process === "undefined" ? 1 : (() => { throw new Error("leak") })()' }, i).detail.error, undefined);
  const escape = family.score({ source: "(item, bin) => bin.constructor.constructor('return typeof process')() === 'undefined' ? 1 : (() => { throw new Error('escape') })()" }, i);
  assert.equal(escape.detail.error, undefined, "the bin's realm has no host Function");
});

test("sandbox mutation of bin, item or ctx does not change a later scoring", () => {
  const [i] = family.instances("mut", 1);
  const before = family.score(firstFit, i);
  const mutating = family.score({ source: "(item, bin, ctx) => { bin.index = 99; bin.remaining = 0; item.size = 1000; ctx.capacity = 1; return -bin.index }" }, i);
  assert.deepEqual(mutating.detail, before.detail, "frozen copies: the mutation is silently ignored");
  assert.deepEqual(family.score(firstFit, i), before);
});

test("controls: unique ids, matching hashes, deterministic random search", () => {
  const opts = { randomSearch: { seed: "a", samples: 6, panel: 2 } };
  const controls = binpackControls(family, opts);
  const ids = controls.map((c) => c.id);
  assert.deepEqual(ids, ["fixed-first-fit", "fixed-best-fit", "fixed-worst-fit", "random-search"]);
  assert.equal(new Set(ids).size, ids.length);
  for (const c of controls) assert.equal(c.package.sha256, sha256(c.package.source), c.id);
  assert.equal(controls[0].package.source, family.root());
  const rs = controls[3];
  assert.deepEqual(rs, binpackControls(family, opts)[3]);
  assert.notEqual(rs.package.source, binpackControls(family, { randomSearch: { seed: "b", samples: 6, panel: 2 } })[3].package.source);
  assert.ok(FEATURES.every((f) => rs.package.source.includes(f)));
  assert.deepEqual(Object.keys(rs.evidence).sort(), ["evaluations", "panel_instances", "samples", "training_mean_progress", "training_success_count"]);
  assert.equal(rs.evidence.evaluations, 12);
});

test("first fit and best fit differ on at least one of 10 instances", () => {
  const instances = family.instances("differ", 10);
  const differ = instances.filter((i) => family.score(firstFit, i).detail.bins !== family.score(bestFit, i).detail.bins);
  assert.ok(differ.length >= 1);
});

test("end to end: one generation on the engine with a verifying ledger", () => {
  const state = createState({ lab: "test", seed: "s" });
  const mutator = { propose: ({ k }) => Array.from({ length: k }, (_, i) => ({ source: `(item, bin) => -bin.fits - ${i + 1} * bin.index` })) };
  const controls = binpackControls(family, { randomSearch: { seed: "t", samples: 4, panel: 2 } });
  const out = run(state, { family, mutator, controls, criterion: { rule: "success-count-margin", min_margin: 1 }, heldout: 3, generations: 1 });
  assert.equal(out.generations, 1);
  const check = verifyLedger(state.ledger);
  assert.equal(check.ok, true, check.reason);
  assert.ok(state.ledger.records.filter((r) => r.type === "evaluation").every((r) => r.outcome === "scored"));
  assert.equal(state.ledger.records.filter((r) => r.type === "verdict").length, 4);
});
