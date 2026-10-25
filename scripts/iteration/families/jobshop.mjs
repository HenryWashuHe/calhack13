// Job-shop dispatching: the first REAL task family for the iteration engine. An instance is a seeded
// jobs x machines job-shop problem (every job visits every machine once, integer durations in [1, 99]);
// a package is JavaScript source that evaluates to a priority rule `(op, ctx) => number`; the score runs
// a greedy dispatcher under that rule and succeeds when the makespan is within `target` of a classic
// lower bound. Deterministic, synchronous, never throws, Node built-ins only, no provider.
import vm from "node:vm";
import { scoreInChild, safeMessage } from "./sandbox.mjs";
import { sha256 } from "../engine.mjs";

export const FEATURES = Object.freeze(["duration", "remainingWork", "remainingOps", "machineRemainingLoad", "waiting"]);

export const featureDoc = [
  "A priority rule is JavaScript source that evaluates to a function (op, ctx) => number; the ready",
  "operation with the HIGHEST number is dispatched first (ties go to the lowest job index).",
  "op fields: job (job index), index (position within the job), machine, duration, remainingWork (this op",
  "plus the rest of its job), remainingOps (count including this op), jobTotalWork, machineRemainingLoad",
  "(durations of all not-yet-started ops on this machine), waiting (time since the job's previous op",
  "finished), readyAt (the time the job's previous op finished; 0 for a first op).",
  "ctx fields: time (current simulated time), machines (count), jobs (count).",
  "The function must return a finite number for every call; anything else fails the instance. The sandbox",
  "has no process, require, imports or timers, op/ctx are frozen copies, and sources containing the tokens",
  "import, async, await or Promise are refused because the score is synchronous.",
].join(" ");

// Seeded draws: the instance string plus a counter through sha256, nothing else.
const draw = (key, counter) => parseInt(sha256(`${key}:${counter}`).slice(0, 8), 16);
const unit = (key, counter) => draw(key, counter) / 0x100000000;
const sum = (xs) => xs.reduce((a, b) => a + b, 0);
const mean = (xs) => (xs.length ? sum(xs) / xs.length : 0);
const swap = (arr, i, j) => arr.with(i, arr[j]).with(j, arr[i]);

function permutation(key, n) {
  const identity = Array.from({ length: n }, (_, i) => i);
  const descending = Array.from({ length: Math.max(0, n - 1) }, (_, k) => n - 1 - k);
  return descending.reduce((arr, i) => swap(arr, i, draw(key, i) % (i + 1)), identity); // Fisher-Yates
}

function lowerBound(jobList, machines) {
  const jobLoad = Math.max(...jobList.map((ops) => sum(ops.map((o) => o.duration))));
  const machineLoad = Math.max(...Array.from({ length: machines }, (_, m) => sum(jobList.flatMap((ops) => ops.filter((o) => o.machine === m).map((o) => o.duration)))));
  return Math.max(jobLoad, machineLoad);
}

/** The problem behind an instance string: each job visits every machine once in a seeded order. */
export function problemOf(instance, jobs, machines) {
  const jobList = Array.from({ length: jobs }, (_, j) => {
    const order = permutation(`${instance}/job/${j}/order`, machines);
    return order.map((machine, i) => ({ machine, duration: 1 + (draw(`${instance}/job/${j}/duration`, i) % 99) }));
  });
  const suffix = jobList.map((ops) => ops.map((_, i) => sum(ops.slice(i).map((o) => o.duration))));
  return { jobs: jobList, machines, suffix, totals: suffix.map((s) => s[0]), lower_bound: lowerBound(jobList, machines) };
}

// ---- greedy dispatcher (pure state transitions) ----
const initialState = (p) => ({
  time: 0,
  next: p.jobs.map(() => 0),
  jobReady: p.jobs.map(() => 0),
  machineFree: Array.from({ length: p.machines }, () => 0),
  machineLoad: Array.from({ length: p.machines }, (_, m) => sum(p.jobs.flatMap((ops) => ops.filter((o) => o.machine === m).map((o) => o.duration)))),
  remaining: sum(p.jobs.map((ops) => ops.length)),
});

const nextOp = (p, s, j) => (s.next[j] < p.jobs[j].length ? { job: j, index: s.next[j], ...p.jobs[j][s.next[j]] } : null);

function readyOps(p, s) {
  return p.jobs.flatMap((_, j) => {
    const o = nextOp(p, s, j);
    return o && s.jobReady[j] <= s.time && s.machineFree[o.machine] <= s.time ? [o] : [];
  });
}

function nextEventTime(p, s) {
  const starts = p.jobs.flatMap((_, j) => { const o = nextOp(p, s, j); return o ? [Math.max(s.jobReady[j], s.machineFree[o.machine])] : []; });
  const t = Math.min(...starts);
  if (!(t > s.time)) throw new Error("dispatcher made no progress");
  return t;
}

const describe = (p, s, o) => ({
  job: o.job, index: o.index, machine: o.machine, duration: o.duration,
  remainingWork: p.suffix[o.job][o.index], remainingOps: p.jobs[o.job].length - o.index, jobTotalWork: p.totals[o.job],
  machineRemainingLoad: s.machineLoad[o.machine], waiting: s.time - s.jobReady[o.job], readyAt: s.jobReady[o.job],
});

function pick(ready, priorityOf) {
  return ready.reduce((best, o) => {
    const value = priorityOf(o);
    if (typeof value !== "number" || !Number.isFinite(value)) throw new Error(`priority returned a non-finite value (${typeof value})`);
    return !best || value > best.value ? { op: o, value } : best; // strict >: ties keep the lower job index
  }, null).op;
}

function start(s, o) {
  const end = s.time + o.duration;
  return { ...s, next: s.next.with(o.job, o.index + 1), jobReady: s.jobReady.with(o.job, end), machineFree: s.machineFree.with(o.machine, end), machineLoad: s.machineLoad.with(o.machine, s.machineLoad[o.machine] - o.duration), remaining: s.remaining - 1 };
}

function step(p, s, priority) {
  const ready = readyOps(p, s);
  if (!ready.length) return { ...s, time: nextEventTime(p, s) };
  const ctx = { time: s.time, machines: p.machines, jobs: p.jobs.length };
  return start(s, pick(ready, (o) => priority(describe(p, s, o), ctx)));
}

/** Makespan of the greedy schedule under `priority(opFeatures, ctx)`. */
export function simulate(p, priority) {
  let s = initialState(p);
  while (s.remaining > 0) s = step(p, s, priority);
  return Math.max(...s.machineFree);
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
  return (op, ctx) => fn(copy(op), copy(ctx));
}

// The dispatch runs under a second vm timeout from a separate context whose only global is the runner, so a
// rule that loops forever is terminated and the rule's realm never holds a host function to climb out of.
const dispatchUnderTimeout = (p, priority, timeout) => vm.runInNewContext("run()", { run: () => simulate(p, priority) }, { timeout });

const failure = (err) => ({ success: false, progress: 0, cost: 1, detail: { error: safeMessage(err) } });

function outcome(p, makespan, target) {
  const ratio = makespan / p.lower_bound;
  return { success: makespan <= target * p.lower_bound, progress: Math.min(1, p.lower_bound / makespan), cost: 1, detail: { makespan, lower_bound: p.lower_bound, ratio } };
}

/** In-process scoring: runs ONLY inside families/rule-child.mjs (empty environment). Never call it from a process that holds secrets. */
export function scoreInProcess(source, instance, { jobs = 10, machines = 5, target = 1.2, timeoutMs = 200 } = {}) {
  const timeout = Math.max(1, Math.floor(timeoutMs));
  try {
    const p = problemOf(instance, jobs, machines);
    const priority = compileRule(source, timeout);
    return outcome(p, dispatchUnderTimeout(p, priority, timeout), target);
  } catch (err) { return failure(err); }
}

export function jobshopFamily({ id, seed, jobs = 10, machines = 5, target = 1.2, timeoutMs = 200 }) {
  const timeout = Math.max(1, Math.floor(timeoutMs));
  return {
    id,
    seed,
    root: () => "(op) => -op.duration",
    instances: (instanceSeed, n) => Array.from({ length: n }, (_, i) => `${id}/${String(instanceSeed).slice(0, 8)}/${i}`),
    // Every rule, including the fixed controls, is scored in a child process with an empty environment (see sandbox.mjs).
    score: (pkg, instance) => scoreInChild({ family: "jobshop", source: pkg && pkg.source, instance, options: { jobs, machines, target }, timeoutMs: timeout }),
  };
}

// ---- controls: three fixed rules and a budgeted random search over linear rules ----
const controlPackage = (source, note) => ({ sha256: sha256(source), source, origin: "control", parent: null, generation: 0, note });
const fixed = (id, source, note) => ({ id, package: controlPackage(source, note) });

export const linearSource = (weights) => `(op) => ${FEATURES.map((f, i) => `(${weights[i].toFixed(2)} * op.${f})`).join(" + ")}`;
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

export function jobshopControls(family, { randomSearch = { seed: "rs", samples: 24, panel: 6 } } = {}) {
  return [
    fixed("fixed-spt", "(op) => -op.duration", "shortest processing time first (equals the family root)"),
    fixed("fixed-mwkr", "(op) => op.remainingWork", "most work remaining first"),
    fixed("fixed-lpt", "(op) => op.duration", "longest processing time first"),
    randomSearchControl(family, { seed: "rs", samples: 24, panel: 6, ...randomSearch }),
  ];
}
