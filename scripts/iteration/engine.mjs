// Domain-free iteration engine: the reference implementation of docs/invention-lab/contracts/iteration.md.
// A domain supplies a Family (seeded instances + deterministic scores) and a Mutator (k candidates from a
// parent and attributed feedback). The engine owns the protocol: candidates before instances, a frozen
// plan before any score, one evaluation per arm per instance, promotion only on a declared margin over
// every control and the incumbent, an append-only chain-hashed ledger, and inheritance that re-verifies.
// No provider is called here; a Mutator may call one. Node only, no dependencies.
import { createHash } from "node:crypto";

export const sha256 = (s) => createHash("sha256").update(String(s)).digest("hex");
const sortKeys = (x) => Array.isArray(x) ? x.map(sortKeys) : x && typeof x === "object" ? Object.fromEntries(Object.keys(x).sort().map((k) => [k, sortKeys(x[k])])) : x;
const canon = (o) => JSON.stringify(sortKeys(o));
const GENESIS = sha256("genesis-iteration/1");

export function createState({ lab, seed }) {
  return { lab, seed: String(seed), generation: 0, frontier: {}, packages: {}, ledger: { records: [], head: GENESIS }, evaluations: 0, calls: 0 };
}

// The ledger is a hash chain: each record carries its index, the previous head and its own hash.
function append(state, record) {
  const body = { ...record, index: state.ledger.records.length, prev: state.ledger.head };
  const hash = sha256(canon(body));
  const stored = { ...body, sha256: hash };
  state.ledger = { records: [...state.ledger.records, stored], head: hash };
  return stored;
}
export function verifyLedger(ledger) {
  let prev = GENESIS;
  for (const [i, r] of (ledger.records || []).entries()) {
    const { sha256: hash, ...body } = r;
    if (body.prev !== prev || body.index !== i) return { ok: false, reason: `record ${i} breaks the chain` };
    if (sha256(canon(body)) !== hash) return { ok: false, reason: `record ${i} was altered` };
    prev = hash;
  }
  return { ok: true, head: prev };
}

export function makePackage(source, { parent = null, origin, generation, note = "" }) {
  return { sha256: sha256(source), source, parent, origin, generation, note };
}
const count = (scores) => scores.filter((s) => s.success).length;

function evaluateArm(state, family, plan, slot, pkg) {
  const scores = plan.instances.map((instance) => ({ instance, ...family.score(pkg, instance) }));
  state.evaluations += scores.length;
  const record = append(state, { type: "evaluation", plan: plan.sha256, slot, package: pkg.sha256, scores, success_count: count(scores), of: scores.length });
  return { slot, package: pkg, scores, success_count: record.success_count };
}

/**
 * One generation on one family. `candidates` (prebuilt packages) may replace `mutator` for inheritance.
 * Returns { plan, verdicts, dropped, promoted }.
 */
export function runGeneration(state, { family, mutator, candidates, k = 4, controls = [], criterion, heldout = 6 }) {
  if (!criterion || criterion.rule !== "success-count-margin" || !(criterion.min_margin >= 1)) throw new Error("criterion must be success-count-margin with min_margin >= 1");
  state.generation += 1;
  const generation = state.generation;
  const frontier = state.frontier[family.id];
  const incumbent = frontier ? frontier.package : makePackage(family.root(), { origin: family.id + ":root", generation: 0, note: "family root" });
  // 1. Candidates are proposed from the parent and its attributed feedback only. No instance id exists yet.
  let proposed;
  if (candidates) proposed = candidates;
  else {
    state.calls += 1;
    proposed = mutator.propose({ parent: incumbent, feedback: frontier ? frontier.feedback : null, k, generation })
      .slice(0, k).map((c, i) => makePackage(c.source, { parent: incumbent.sha256, origin: state.lab, generation, note: c.note || `candidate ${i}` }));
  }
  const excluded = new Set([incumbent.sha256, ...controls.map((c) => c.package.sha256)]);
  const seen = new Set();
  const dropped = [], kept = [];
  for (const p of proposed) {
    if (excluded.has(p.sha256)) dropped.push({ package: p.sha256, reason: "identical to the incumbent or a control" });
    else if (seen.has(p.sha256)) dropped.push({ package: p.sha256, reason: "duplicate candidate" });
    else { seen.add(p.sha256); kept.push(p); }
  }
  // A package keeps the provenance of its first appearance; re-proposing known bytes never rewrites lineage.
  for (const p of kept) if (!state.packages[p.sha256]) state.packages = { ...state.packages, [p.sha256]: p };
  append(state, { type: "candidates_proposed", generation, family: family.id, parent: incumbent.sha256, candidates: kept.map((p) => p.sha256), dropped });
  if (!kept.length) return { plan: null, verdicts: [], dropped, promoted: null };
  // 2. The plan freezes arms, criterion and the held-out instances before any score exists.
  const instanceSeed = sha256(`${state.seed}:${family.id}:${generation}`);
  const instances = family.instances(instanceSeed, heldout);
  const arms = [{ slot: "incumbent", package: incumbent.sha256 }, ...controls.map((c) => ({ slot: "control:" + c.id, package: c.package.sha256 })), ...kept.map((p, i) => ({ slot: "candidate:" + i, package: p.sha256 }))];
  const planBody = { family: family.id, generation, arms, criterion, instances, instance_seed: instanceSeed };
  const plan = { ...planBody, sha256: sha256(canon(planBody)) };
  append(state, { type: "plan_frozen", plan: plan.sha256, ...planBody });
  // 3. Exactly one evaluation per arm per instance, in plan order. No reruns, no selection among runs.
  const inc = evaluateArm(state, family, plan, "incumbent", incumbent);
  const ctrl = controls.map((c) => evaluateArm(state, family, plan, "control:" + c.id, c.package));
  const cands = kept.map((p, i) => evaluateArm(state, family, plan, "candidate:" + i, p));
  // 4. A verdict for every candidate against the incumbent and every control, by the declared margin.
  const verdicts = cands.map((c) => {
    const margins = [inc, ...ctrl].map((o) => ({ against: o.slot, candidate: c.success_count, other: o.success_count, margin: c.success_count - o.success_count }));
    const promotable = margins.every((m) => m.margin >= criterion.min_margin);
    const reason = margins.map((m) => `${m.candidate}/${plan.instances.length} vs ${m.against} ${m.other}/${plan.instances.length}`).join("; ") + ` (success-count margin >= ${criterion.min_margin} required)`;
    return { package: c.package, slot: c.slot, success_count: c.success_count, scores: c.scores, margins, promotable, reason };
  });
  // 5. At most one promotion: the best promotable candidate; ties among the best go to plan order.
  const best = verdicts.filter((v) => v.promotable).sort((a, b) => b.success_count - a.success_count)[0] || null;
  const decided = verdicts.map((v) => ({ ...v, verdict: v === best ? "promoted" : "rejected" }));
  for (const v of decided) append(state, { type: "verdict", plan: plan.sha256, generation, slot: v.slot, package: v.package.sha256, verdict: v.verdict, reason: v.reason, margins: v.margins });
  const feedbackFor = (arm) => ({ plan: plan.sha256, instances: arm.scores });
  if (best) {
    append(state, { type: "promotion", generation, family: family.id, package: best.package.sha256, parent: incumbent.sha256, success_count: best.success_count, incumbent_count: inc.success_count, of: plan.instances.length, plan: plan.sha256 });
    state.frontier = { ...state.frontier, [family.id]: { package: best.package, success_count: best.success_count, plan: plan.sha256, generation, feedback: feedbackFor(best) } };
  } else if (frontier) {
    state.frontier = { ...state.frontier, [family.id]: { ...frontier, feedback: feedbackFor(inc) } }; // fresh attribution for the next generation
  } else {
    state.frontier = { ...state.frontier }; // no frontier yet; the root stays the incumbent (its feedback is carried below)
    state.rootFeedback = { ...(state.rootFeedback || {}), [family.id]: feedbackFor(inc) };
  }
  return { plan, verdicts: decided, dropped, promoted: best ? best.package : null };
}

/** The loop with stop rules: a generation count, an evaluation budget, or a stall (no promotion in N generations). */
export function run(state, { family, mutator, k = 4, controls = [], criterion, heldout = 6, generations = 10, budget = {}, stall = Infinity }) {
  let promotions = 0, sinceLast = 0, done = 0, stopped = "generations";
  for (let g = 0; g < generations; g++) {
    const cost = (k + controls.length + 1) * heldout;
    if (budget.evaluations !== undefined && state.evaluations + cost > budget.evaluations) { stopped = "budget"; break; }
    if (budget.calls !== undefined && state.calls + 1 > budget.calls) { stopped = "budget"; break; }
    const frontier = state.frontier[family.id];
    const feedback = frontier ? null : (state.rootFeedback || {})[family.id] || null;
    const wrapped = feedback && !frontier ? { propose: (a) => mutator.propose({ ...a, feedback }) } : mutator;
    const out = runGeneration(state, { family, mutator: wrapped, k, controls, criterion, heldout });
    done += 1;
    if (out.promoted) { promotions += 1; sinceLast = 0; } else if (++sinceLast >= stall) { stopped = "stall"; break; }
  }
  append(state, { type: "run_stopped", family: family.id, reason: stopped, generations: done, promotions, evaluations: state.evaluations, calls: state.calls });
  return { generations: done, promotions, stopped };
}

/**
 * Confirmation: score the frontier once on a final fresh panel whose seed was never used for selection.
 * Recorded, never fed back, never used to choose. This is the only count a frontier claim should quote.
 */
export function confirm(state, { family, heldout = 6, panelSeed }) {
  const f = state.frontier[family.id];
  if (!f) throw new Error(`no frontier for ${family.id}`);
  if (panelSeed === undefined) throw new Error("confirm needs a panelSeed that was never used for selection");
  const instanceSeed = sha256(`confirm:${panelSeed}:${family.id}`);
  const used = state.ledger.records.some((r) => r.type === "plan_frozen" && r.instance_seed === instanceSeed);
  if (used) throw new Error("that panel was already used for selection");
  const instances = family.instances(instanceSeed, heldout);
  const scores = instances.map((instance) => ({ instance, ...family.score(f.package, instance) }));
  state.evaluations += scores.length;
  const record = append(state, { type: "confirmation", family: family.id, package: f.package.sha256, selection_plan: f.plan, instance_seed: instanceSeed, instances, scores, success_count: count(scores), of: scores.length });
  return { package: f.package.sha256, success_count: record.success_count, of: record.of, selection_count: f.success_count };
}

/** Passing on: the frontier package, its ancestry and the whole ledger, hash-bound. */
export function exportBundle(state, familyId) {
  const f = state.frontier[familyId];
  if (!f) throw new Error(`no frontier for ${familyId}`);
  const lineage = [], visited = new Set();
  for (let p = state.packages[f.package.sha256] || f.package; p && !visited.has(p.sha256); p = p.parent ? state.packages[p.parent] : null) {
    visited.add(p.sha256); lineage.push({ sha256: p.sha256, origin: p.origin, generation: p.generation, note: p.note });
  }
  const ledger = { records: state.ledger.records, head: state.ledger.head };
  return { package: f.package, family: familyId, success_count: f.success_count, plan: f.plan, lineage, ledger, ledger_sha256: sha256(JSON.stringify(ledger.records)), exported_by: state.lab };
}

/** Adoption never trusts the sender's numbers: the package is re-verified on the adopter's own instances. */
export function adopt(state, { bundle, family, controls = [], criterion, heldout = 6 }) {
  const check = verifyLedger(bundle.ledger);
  if (!check.ok || sha256(JSON.stringify(bundle.ledger.records)) !== bundle.ledger_sha256) {
    append(state, { type: "adoption", family: family.id, package: bundle.package.sha256, from: bundle.exported_by, verdict: "rejected", reason: "bundle ledger does not verify: " + (check.reason || "hash mismatch") });
    return { verdict: "rejected", reason: "bundle ledger does not verify" };
  }
  if (sha256(bundle.package.source) !== bundle.package.sha256) return { verdict: "rejected", reason: "package hash does not match its source" };
  const candidate = { ...bundle.package, note: `adopted from ${bundle.exported_by}; ${bundle.package.note || ""}`.trim() };
  state.packages = { ...state.packages, [candidate.sha256]: candidate };
  const out = runGeneration(state, { family, candidates: [candidate], controls, criterion, heldout });
  const v = out.verdicts[0] || { verdict: "rejected", reason: "identical to the local incumbent or a control" };
  append(state, { type: "adoption", family: family.id, package: candidate.sha256, from: bundle.exported_by, from_ledger_sha256: bundle.ledger_sha256, lineage: bundle.lineage.map((l) => l.sha256), verdict: v.verdict, reason: v.reason });
  return { verdict: v.verdict, reason: v.reason };
}
