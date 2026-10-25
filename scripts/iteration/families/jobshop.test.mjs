// Tests for the job-shop family: instance determinism, scoring, sandbox containment, controls and one
// end-to-end generation on the engine. Run: node --test scripts/iteration/families/jobshop.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import { createState, run, verifyLedger, sha256 } from "../engine.mjs";
import { jobshopFamily, jobshopControls, problemOf, FEATURES } from "./jobshop.mjs";

const family = jobshopFamily({ id: "jobshop", seed: "s" });
const pkgOf = (source) => ({ sha256: sha256(source), source });
const spt = pkgOf(family.root());
const mwkr = pkgOf("(op) => op.remainingWork");

test("instances are n distinct prefixed strings, identical for the same seed", () => {
  const a = family.instances("abcdefgh12345", 5), b = family.instances("abcdefgh12345", 5);
  assert.equal(a.length, 5);
  assert.equal(new Set(a).size, 5);
  assert.ok(a.every((i) => i.startsWith("jobshop/abcdefgh/")));
  assert.deepEqual(a, b);
  assert.notDeepEqual(a, family.instances("zzzzzzzz", 5));
});

test("instance content: each job visits every machine exactly once with durations in [1, 99]", () => {
  const p = problemOf("jobshop/x/0", 10, 5);
  assert.equal(p.jobs.length, 10);
  for (const ops of p.jobs) {
    assert.deepEqual([...ops.map((o) => o.machine)].sort(), [0, 1, 2, 3, 4]);
    assert.ok(ops.every((o) => Number.isInteger(o.duration) && o.duration >= 1 && o.duration <= 99));
  }
  assert.deepEqual(problemOf("jobshop/x/0", 10, 5), p);
});

test("score is deterministic", () => {
  const [i] = family.instances("det", 1);
  assert.deepEqual(family.score(spt, i), family.score(spt, i));
  assert.equal(typeof family.score(spt, i).success, "boolean");
});

test("lower bound never exceeds the makespan (root on 5 instances)", () => {
  for (const i of family.instances("lb", 5)) {
    const { detail } = family.score(spt, i);
    assert.ok(detail.lower_bound <= detail.makespan, `${i}: ${detail.lower_bound} > ${detail.makespan}`);
    assert.ok(detail.ratio >= 1);
  }
});

test("malformed, non-function and throwing sources fail without throwing", () => {
  const [i] = family.instances("bad", 1);
  for (const source of ["(op) =>", "42", '{ "guess": 0 }', "(op) => { throw new Error('boom') }", "(op) => NaN", "(op) => 'high'", undefined]) {
    const r = family.score({ source }, i);
    assert.equal(r.success, false, String(source));
    assert.equal(r.progress, 0);
    assert.equal(typeof r.detail.error, "string");
  }
  assert.equal(family.score(null, i).success, false);
});

test("an infinite loop at evaluation or inside the rule returns within about 3x the timeout", () => {
  const quick = jobshopFamily({ id: "jobshop", seed: "s", timeoutMs: 100 });
  const [i] = quick.instances("loop", 1);
  for (const source of ["(() => { while (true) {} })()", "(op) => { while (true) {} }"]) {
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
  const r = family.score({ source: '(op) => typeof process === "undefined" ? 1 : (() => { throw new Error("leak") })()' }, i);
  assert.equal(typeof r.success, "boolean");
  assert.equal(r.detail.error, undefined);
  for (const name of ["require", "setTimeout", "setInterval"]) {
    assert.equal(family.score({ source: `(op) => { ${name}; return 1 }` }, i).success, false, name);
  }
  for (const source of ["(op) => { import('node:fs'); return 1 }", "(op) => { Promise.reject(new Error('x')); return 1 }", "(op) => { (async () => { throw new Error('y') })(); return 1 }"]) {
    const r = family.score({ source }, i); // a rejected sandbox promise would later crash the host: refused before evaluation
    assert.equal(r.success, false, source);
    assert.match(r.detail.error, /not allowed/);
  }
  assert.equal(family.score({ source: '(op) => typeof globalThis.process === "undefined" ? 1 : (() => { throw new Error("leak") })()' }, i).detail.error, undefined);
  const escape = family.score({ source: "(op) => op.constructor.constructor('return typeof process')() === 'undefined' ? 1 : (() => { throw new Error('escape') })()" }, i);
  assert.equal(escape.detail.error, undefined, "the op's realm has no host Function");
});

test("sandbox mutation of op does not change a later scoring", () => {
  const [i] = family.instances("mut", 1);
  const before = family.score(spt, i);
  const mutating = family.score({ source: "(op) => { op.duration = 1000; op.remainingWork = 0; return -op.duration }" }, i);
  assert.deepEqual(mutating.detail, before.detail, "frozen copies: the mutation is silently ignored");
  assert.deepEqual(family.score(spt, i), before);
});

test("controls: unique ids, matching hashes, deterministic random search", () => {
  const opts = { randomSearch: { seed: "a", samples: 6, panel: 2 } };
  const controls = jobshopControls(family, opts);
  const ids = controls.map((c) => c.id);
  assert.deepEqual(ids, ["fixed-spt", "fixed-mwkr", "fixed-lpt", "random-search"]);
  assert.equal(new Set(ids).size, ids.length);
  for (const c of controls) assert.equal(c.package.sha256, sha256(c.package.source), c.id);
  const rs = controls[3];
  assert.deepEqual(rs, jobshopControls(family, opts)[3]);
  assert.notEqual(rs.package.source, jobshopControls(family, { randomSearch: { seed: "b", samples: 6, panel: 2 } })[3].package.source);
  assert.ok(FEATURES.every((f) => rs.package.source.includes(`op.${f}`)));
  assert.deepEqual(Object.keys(rs.evidence).sort(), ["evaluations", "panel_instances", "samples", "training_mean_progress", "training_success_count"]);
  assert.equal(rs.evidence.evaluations, 12);
});

test("SPT and MWKR differ on at least one of 10 instances", () => {
  const instances = family.instances("diff", 10);
  const differ = instances.filter((i) => family.score(spt, i).detail.makespan !== family.score(mwkr, i).detail.makespan);
  assert.ok(differ.length >= 1);
});

test("end to end: one generation on the engine with a verifying ledger", () => {
  const state = createState({ lab: "test", seed: "s" });
  const mutator = { propose: ({ k }) => Array.from({ length: k }, (_, i) => ({ source: `(op) => op.remainingWork - ${i} * op.duration` })) };
  const controls = jobshopControls(family, { randomSearch: { seed: "t", samples: 4, panel: 2 } });
  const out = run(state, { family, mutator, controls, criterion: { rule: "success-count-margin", min_margin: 1 }, heldout: 3, generations: 1 });
  assert.equal(out.generations, 1);
  const check = verifyLedger(state.ledger);
  assert.equal(check.ok, true, check.reason);
  assert.ok(state.ledger.records.filter((r) => r.type === "evaluation").every((r) => r.outcome === "scored"));
  assert.equal(state.ledger.records.filter((r) => r.type === "verdict").length, 4);
});
