// Online bin packing: the second REAL task family for the iteration engine. An instance is a seeded
// sequence of integer item sizes in [1, 50] arriving in order; a package is JavaScript source that
// evaluates to a priority rule `(item, bin, ctx) => number` choosing, among the open bins that still fit
// the item, the one with the HIGHEST value (a new bin is opened only when none fits). The score succeeds
// when the number of bins used is within `target` of the volume lower bound ceil(sum / capacity).
// Deterministic, synchronous, never throws, Node built-ins only, no provider.
import vm from "node:vm";
import { sha256 } from "../engine.mjs";

export const FEATURES = Object.freeze(["bin.remaining", "bin.fits", "bin.fill", "bin.index", "item.size"]);

export const featureDoc = [
  "A priority rule is JavaScript source that evaluates to a function (item, bin, ctx) => number. Items",
  "arrive one at a time; for each item the rule is called once per OPEN bin that still fits it, and the",
  "bin with the HIGHEST number receives the item (ties go to the lowest bin index). If no open bin fits,",
  "a new bin is opened without calling the rule.",
  "item fields: size (integer in [1, 50]), index (arrival position, 0-based), remainingItems (items still",
  "to arrive after this one).",
  "bin fields: index (opening order, 0-based), remaining (free capacity before this item), fill (used /",
  "capacity, in [0, 1)), count (items already in the bin), fits (remaining - item.size; >= 0 for every bin",
  "the rule sees).",
  "ctx fields: capacity (bin capacity), openBins (number of open bins), itemsLeft (items not yet placed,",
  "this one included).",
  "The function must return a finite number for every call; anything else fails the instance. The sandbox",
  "has no process, require, imports or timers, item/bin/ctx are frozen copies, and sources containing the",
  "tokens import, async, await or Promise are refused because the score is synchronous.",
].join(" ");

// Seeded draws: the instance string plus a counter through sha256, nothing else.
const draw = (key, counter) => parseInt(sha256(`${key}:${counter}`).slice(0, 8), 16);
const unit = (key, counter) => draw(key, counter) / 0x100000000;
const sum = (xs) => xs.reduce((a, b) => a + b, 0);
const mean = (xs) => (xs.length ? sum(xs) / xs.length : 0);

const DEFAULT_SIZES = Object.freeze([1, 50]);

/** The problem behind an instance string: `items` integer sizes in [min, max] (default [1, 50]) and the volume lower bound. */
export function problemOf(instance, items, capacity, sizeRange = DEFAULT_SIZES) {
  const [lo, hi] = sizeRange;
  const sizes = Array.from({ length: items }, (_, i) => lo + (draw(`${instance}/size`, i) % (hi - lo + 1)));
  const total = sum(sizes);
  return { sizes, capacity, total, lower_bound: Math.ceil(total / capacity) };
}

// ---- online packer (pure state transitions) ----
const describeItem = (p, i) => ({ size: p.sizes[i], index: i, remainingItems: p.sizes.length - i - 1 });

const describeBin = (p, bin, index, size) => ({
  index, remaining: bin.remaining, fill: (p.capacity - bin.remaining) / p.capacity, count: bin.count, fits: bin.remaining - size,
});

const feasible = (bins, size) => bins.flatMap((bin, index) => (bin.remaining >= size ? [{ bin, index }] : []));

function pick(candidates, priorityOf) {
  return candidates.reduce((best, c) => {
    const value = priorityOf(c);
    if (typeof value !== "number" || !Number.isFinite(value)) throw new Error(`priority returned a non-finite value (${typeof value})`);
    return !best || value > best.value ? { index: c.index, value } : best; // strict >: ties keep the lower bin index
  }, null).index;
}

const placeIn = (bins, index, size) => bins.with(index, { remaining: bins[index].remaining - size, count: bins[index].count + 1 });
const openWith = (bins, capacity, size) => [...bins, { remaining: capacity - size, count: 1 }];

function place(p, bins, i, priority) {
  const size = p.sizes[i];
  const open = feasible(bins, size);
  if (!open.length) return openWith(bins, p.capacity, size);
  const item = describeItem(p, i);
  const ctx = { capacity: p.capacity, openBins: bins.length, itemsLeft: p.sizes.length - i };
  return placeIn(bins, pick(open, (c) => priority(item, describeBin(p, c.bin, c.index, size), ctx)), size);
}

/** Bins used by the online packing under `priority(itemFeatures, binFeatures, ctx)`. */
export function simulate(p, priority) {
  return p.sizes.reduce((bins, _, i) => place(p, bins, i, priority), []).length;
}

// ---- sandbox: the rule's realm never sees the host's objects or functions ----
// A promise rejected inside the sandbox (import(), Promise.reject, an async throw) would surface later as an
// unhandled rejection in the HOST process, which node:vm cannot intercept without an experimental flag; the
// score is synchronous, so sources that spell those tokens are refused before evaluation.
const ASYNC_TOKENS = /\b(import|async|await|Promise)\b/;

function compileRule(source, timeout) {
  const text = String(source);
  if (ASYNC_TOKENS.test(text)) throw new Error("asynchronous code (import, async, await, Promise) is not allowed in a priority rule");
  const realm = vm.createContext({});
  const fn = vm.runInContext(text, realm, { timeout });
  if (typeof fn !== "function") throw new Error(`source must evaluate to a function, got ${typeof fn}`);
  const copy = vm.runInContext("(o) => Object.freeze(Object.assign({}, o))", realm); // frozen copies born in the rule's realm
  return (item, bin, ctx) => fn(copy(item), copy(bin), copy(ctx));
}

// The packing runs under a second vm timeout from a separate context whose only global is the runner, so a
// rule that loops forever is terminated and the rule's realm never holds a host function to climb out of.
const packUnderTimeout = (p, priority, timeout) => vm.runInNewContext("run()", { run: () => simulate(p, priority) }, { timeout });

function shortMessage(err) {
  try {
    const m = err && typeof err.message === "string" ? err.message : String(err);
    return m.split("\n")[0].slice(0, 160);
  } catch { return "unknown error"; }
}

const failure = (err) => ({ success: false, progress: 0, cost: 1, detail: { error: shortMessage(err) } });

function outcome(p, bins, target) {
  const ratio = bins / p.lower_bound;
  return { success: bins <= target * p.lower_bound, progress: Math.min(1, p.lower_bound / bins), cost: 1, detail: { bins, lower_bound: p.lower_bound, ratio } };
}

export function binpackFamily({ id, seed, items = 100, capacity = 100, target = 1.02, timeoutMs = 200, sizes = DEFAULT_SIZES }) {
  if (!Array.isArray(sizes) || sizes.length !== 2 || !(Number.isInteger(sizes[0]) && Number.isInteger(sizes[1]) && sizes[0] >= 1 && sizes[1] >= sizes[0] && sizes[1] <= capacity)) throw new Error("sizes must be [min, max] integers within 1..capacity");
  const timeout = Math.max(1, Math.floor(timeoutMs));
  return {
    id,
    seed,
    root: () => "(item, bin) => -bin.index",
    instances: (instanceSeed, n) => Array.from({ length: n }, (_, i) => `${id}/${String(instanceSeed).slice(0, 8)}/${i}`),
    score: (pkg, instance) => {
      try {
        const p = problemOf(instance, items, capacity, sizes);
        const priority = compileRule(pkg && pkg.source, timeout);
        return outcome(p, packUnderTimeout(p, priority, timeout), target);
      } catch (err) { return failure(err); }
    },
  };
}

// ---- controls: three fixed rules and a budgeted random search over linear rules ----
const controlPackage = (source, note) => ({ sha256: sha256(source), source, origin: "control", parent: null, generation: 0, note });
const fixed = (id, source, note) => ({ id, package: controlPackage(source, note) });

export const linearSource = (weights) => `(item, bin) => ${FEATURES.map((f, i) => `(${weights[i].toFixed(2)} * ${f})`).join(" + ")}`;
const sampleWeights = (seed, s) => FEATURES.map((_, f) => Math.round((unit(`${seed}/random-search/${s}`, f) * 2 - 1) * 100) / 100);

function trainingScore(family, source, panel) {
  const results = panel.map((instance) => family.score({ sha256: sha256(source), source }, instance));
  return { source, successes: results.filter((r) => r.success).length, meanProgress: mean(results.map((r) => r.progress)) };
}
const better = (a, b) => (a.successes !== b.successes ? a.successes > b.successes : a.meanProgress > b.meanProgress); // ties keep the earlier sample

function randomSearchControl(family, { seed, samples, panel }) {
  const training = family.instances(sha256("control-training:" + seed), panel); // private to the control; never a selection panel
  const scored = Array.from({ length: samples }, (_, s) => trainingScore(family, linearSource(sampleWeights(seed, s)), training));
  const best = scored.reduce((b, r) => (better(r, b) ? r : b));
  return {
    id: "random-search",
    package: controlPackage(best.source, `best of ${samples} seeded random linear rules on a private ${panel}-instance training panel`),
    evidence: { samples, panel_instances: panel, training_success_count: best.successes, training_mean_progress: best.meanProgress, evaluations: samples * panel },
  };
}

export function binpackControls(family, { randomSearch = { seed: "rs", samples: 24, panel: 6 } } = {}) {
  return [
    fixed("fixed-first-fit", "(item, bin) => -bin.index", "first fit: the lowest-index open bin that fits (equals the family root)"),
    fixed("fixed-best-fit", "(item, bin) => -bin.fits", "best fit: the open bin the item fits most tightly"),
    fixed("fixed-worst-fit", "(item, bin) => bin.fits", "worst fit: the open bin with the most room left after the item"),
    randomSearchControl(family, { seed: "rs", samples: 24, panel: 6, ...randomSearch }),
  ];
}
