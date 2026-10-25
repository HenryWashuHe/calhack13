// Domain-free iteration engine: the executable reference of docs/invention-lab/contracts/iteration.md
// (a proposal under interface review; not an orchestrator; its records do not replace the retained
// contracts). A domain supplies a Family (seeded instances + deterministic scores) and a Mutator
// (k candidates from a parent and attributed, id-free feedback). The engine owns the protocol:
// candidates before instances, a frozen plan before any score, one evaluation per arm per instance,
// strict boolean scores (anything else is an unknown outcome, never a verdict), promotion only on a
// declared margin over every control and the incumbent, refusals that spend nothing, abandoned plans
// that are retained and resumed rather than redrawn, an append-only head-bound canonical hash chain,
// inheritance that re-verifies on the adopter's own lab-scoped panels, and a once-only budgeted
// confirmation on a panel disjoint from every selection panel. Node only, no dependencies, no provider.
import { createHash } from "node:crypto";

export const sha256 = (s) => createHash("sha256").update(String(s)).digest("hex");

// Canonical serialization: sorted keys, undefined dropped, and numbers that JSON would collapse
// (-0, Infinity, NaN) encoded explicitly so an edit from Infinity to null changes the hash.
export function canon(x) {
  if (typeof x === "number") return Object.is(x, -0) ? '{"$n":"-0"}' : Number.isFinite(x) ? JSON.stringify(x) : `{"$n":"${String(x)}"}`;
  if (x === undefined) return "null";
  if (x === null || typeof x !== "object") return JSON.stringify(x);
  if (Array.isArray(x)) return "[" + x.map(canon).join(",") + "]";
  return "{" + Object.keys(x).filter((k) => x[k] !== undefined).sort().map((k) => JSON.stringify(k) + ":" + canon(x[k])).join(",") + "}";
}
const freeze = (x) => { if (x && typeof x === "object" && !Object.isFrozen(x)) { Object.freeze(x); for (const v of Object.values(x)) freeze(v); } return x; };
const GENESIS = sha256("genesis-iteration/1");
const HEX64 = /^[0-9a-f]{64}$/;

export function createState({ lab, seed }) {
  if (typeof lab !== "string" || !lab) throw new Error("createState needs a lab identity");
  return { lab, seed: String(seed), generation: 0, frontier: {}, packages: {}, pending: {}, ledger: { records: [], head: GENESIS }, evaluations: 0, calls: 0 };
}

// Every record carries the lab, its index, the previous head and its own hash over the canonical body.
function append(state, record) {
  const body = { ...record, lab: state.lab, index: state.ledger.records.length, prev: state.ledger.head };
  const hash = sha256(canon(body));
  const stored = freeze({ ...body, sha256: hash });
  state.ledger = { records: [...state.ledger.records, stored], head: hash };
  return stored;
}
/** Verifies the chain record by record and binds the declared head to the recomputed one. */
export function verifyLedger(ledger) {
  if (!ledger || !Array.isArray(ledger.records)) return { ok: false, reason: "no records" };
  let prev = GENESIS;
  for (const [i, r] of ledger.records.entries()) {
    if (!r || typeof r !== "object") return { ok: false, reason: `record ${i} is not an object` };
    const { sha256: hash, ...body } = r;
    if (body.prev !== prev || body.index !== i) return { ok: false, reason: `record ${i} breaks the chain` };
    if (sha256(canon(body)) !== hash) return { ok: false, reason: `record ${i} was altered` };
    prev = hash;
  }
  if (ledger.head !== prev) return { ok: false, reason: "declared head does not match the chain" };
  return { ok: true, head: prev };
}

export function makePackage(source, { parent = null, origin, generation, note = "" }) {
  return { sha256: sha256(source), source, parent, origin, generation, note };
}
function register(state, pkg) {
  // First provenance wins: known bytes never get a new parent, origin or generation.
  if (!state.packages[pkg.sha256]) state.packages = { ...state.packages, [pkg.sha256]: freeze({ ...pkg }) };
  return state.packages[pkg.sha256];
}
function rootPackage(state, family) {
  const root = makePackage(family.root(), { origin: family.id + ":root", generation: 0, note: "family root" });
  if (!state.packages[root.sha256]) { register(state, root); append(state, { type: "root_registered", family: family.id, package: root.sha256 }); }
  return state.packages[root.sha256];
}

const validScore = (s) => !!s && typeof s === "object" && typeof s.success === "boolean";
const scoredCount = (scores) => scores.filter((s) => s.outcome === "scored" && s.success === true).length;
const armValid = (arm) => arm.outcome === "scored" && arm.scores.every((s) => s.outcome === "scored");
const idFree = (scores) => scores.map(({ instance, ...rest }, index) => ({ index, ...rest })); // the mutator never sees instance ids

/** One arm, one instance at a time, in plan order. A throw is recorded as a failed outcome, counted, then rethrown. */
function evaluateArm(state, family, plan, slot, pkg) {
  const scores = [];
  for (const instance of plan.instances) {
    state.evaluations += 1;
    let raw;
    try { raw = family.score(pkg, instance); } catch (err) {
      scores.push({ instance, outcome: "failed", reason: err && err.message ? err.message : "scorer threw" });
      append(state, { type: "evaluation", plan: plan.sha256, slot, package: pkg.sha256, scores, outcome: "failed", success_count: scoredCount(scores), of: plan.instances.length, reason: scores.at(-1).reason });
      throw err;
    }
    scores.push(validScore(raw) ? { instance, outcome: "scored", success: raw.success, progress: raw.progress, cost: raw.cost, detail: raw.detail } : { instance, outcome: "invalid", reason: "score must carry a boolean success" });
  }
  const outcome = scores.every((s) => s.outcome === "scored") ? "scored" : "invalid";
  append(state, { type: "evaluation", plan: plan.sha256, slot, package: pkg.sha256, scores, outcome, success_count: scoredCount(scores), of: scores.length });
  return { slot, package: pkg, scores, outcome, success_count: scoredCount(scores) };
}

function refuse(state, family, generation, reason) {
  append(state, { type: "generation_refused", family: family.id, generation, reason });
  return { plan: null, verdicts: [], dropped: [], promoted: null, refused: reason };
}

/**
 * One generation on one family. `candidates` (prebuilt packages) may replace `mutator` for inheritance.
 * A refused generation records why and spends nothing. An abandoned generation (scorer threw) keeps its
 * frozen plan and completed arms in `state.pending` and is resumed by the next call for that family.
 */
export function runGeneration(state, { family, mutator, candidates, k = 4, controls = [], criterion, heldout = 6 }) {
  if (!criterion || criterion.rule !== "success-count-margin" || !(Number.isInteger(criterion.min_margin) && criterion.min_margin >= 1)) throw new Error("criterion must be success-count-margin with an integer min_margin >= 1");
  const pending = state.pending[family.id];
  let plan, incumbent, ctrlPkgs, kept, dropped = [], results;
  if (pending) {
    ({ plan, incumbent, controls: ctrlPkgs, kept, results } = pending);
    append(state, { type: "generation_resumed", family: family.id, generation: plan.generation, plan: plan.sha256, completed: Object.keys(results) });
  } else {
    if (!candidates && !(Number.isInteger(k) && k >= 1)) return refuse(state, family, state.generation + 1, "k must be a positive integer");
    if (!(Number.isInteger(heldout) && heldout >= 1)) return refuse(state, family, state.generation + 1, "heldout must be a positive integer");
    state.generation += 1;
    const generation = state.generation;
    const frontier = state.frontier[family.id];
    incumbent = frontier ? frontier.package : rootPackage(state, family);
    // 1. Candidates come from the parent and id-free attributed feedback only; no instance exists yet.
    let proposed;
    if (candidates) proposed = candidates.map((c) => register(state, c));
    else {
      state.calls += 1;
      const feedback = frontier ? frontier.feedback : (state.rootFeedback || {})[family.id] || null;
      const out = mutator.propose({ parent: incumbent, feedback, k, generation });
      proposed = (Array.isArray(out) ? out : []).slice(0, k).map((c, i) => register(state, makePackage(c.source, { parent: incumbent.sha256, origin: state.lab, generation, note: c.note || `candidate ${i}` })));
    }
    ctrlPkgs = controls.map((c) => ({ id: c.id, package: register(state, c.package) }));
    const excluded = new Set([incumbent.sha256, ...ctrlPkgs.map((c) => c.package.sha256)]);
    const seen = new Set(); kept = [];
    for (const p of proposed) {
      if (excluded.has(p.sha256)) dropped.push({ package: p.sha256, reason: "identical to the incumbent or a control" });
      else if (seen.has(p.sha256)) dropped.push({ package: p.sha256, reason: "duplicate candidate" });
      else { seen.add(p.sha256); kept.push(p); }
    }
    append(state, { type: "candidates_proposed", generation, family: family.id, parent: incumbent.sha256, candidates: kept.map((p) => p.sha256), dropped });
    if (!kept.length) return { plan: null, verdicts: [], dropped, promoted: null };
    // 2. The plan freezes arms, criterion and the lab-scoped held-out panel before any score exists.
    const instanceSeed = sha256(`${state.lab}:${state.seed}:${family.id}:${generation}`);
    const instances = family.instances(instanceSeed, heldout);
    if (!Array.isArray(instances) || instances.length !== heldout || instances.some((i) => typeof i !== "string" || !i) || new Set(instances).size !== instances.length) {
      return refuse(state, family, generation, `family returned ${Array.isArray(instances) ? instances.length : "no"} instances against ${heldout} declared`);
    }
    const arms = [{ slot: "incumbent", package: incumbent.sha256 }, ...ctrlPkgs.map((c) => ({ slot: "control:" + c.id, package: c.package.sha256 })), ...kept.map((p, i) => ({ slot: "candidate:" + i, package: p.sha256 }))];
    const planBody = { family: family.id, generation, arms, criterion: { rule: criterion.rule, min_margin: criterion.min_margin }, instances, instance_seed: instanceSeed };
    plan = freeze({ ...planBody, sha256: sha256(canon(planBody)) });
    append(state, { type: "plan_frozen", plan: plan.sha256, ...planBody });
    results = {};
  }
  // 3. Exactly one evaluation per arm per instance, in plan order; completed arms of a resumed plan are kept.
  const order = [["incumbent", incumbent], ...ctrlPkgs.map((c) => ["control:" + c.id, c.package]), ...kept.map((p, i) => ["candidate:" + i, p])];
  for (const [slot, pkg] of order) {
    if (results[slot]) continue;
    try { results = { ...results, [slot]: evaluateArm(state, family, plan, slot, pkg) }; }
    catch (err) {
      state.pending = { ...state.pending, [family.id]: { plan, incumbent, controls: ctrlPkgs, kept, results } };
      append(state, { type: "generation_abandoned", family: family.id, generation: plan.generation, plan: plan.sha256, at: slot, reason: err && err.message ? err.message : "scorer threw", completed: Object.keys(results) });
      throw err;
    }
  }
  state.pending = Object.fromEntries(Object.entries(state.pending).filter(([f]) => f !== family.id));
  const inc = results.incumbent, ctrl = ctrlPkgs.map((c) => results["control:" + c.id]), cands = kept.map((p, i) => results["candidate:" + i]);
  // 4. A verdict for every candidate: insufficient when any compared outcome is unknown, else by margin.
  const verdicts = cands.map((c) => {
    const compared = [inc, ...ctrl];
    const unknown = !armValid(c) || compared.some((o) => !armValid(o));
    const margins = compared.map((o) => ({ against: o.slot, candidate: c.success_count, other: o.success_count, margin: c.success_count - o.success_count, known: armValid(o) }));
    const promotable = !unknown && margins.every((m) => m.margin >= plan.criterion.min_margin);
    const reason = unknown
      ? `insufficient: ${[c, ...compared].filter((a) => !armValid(a)).map((a) => a.slot + " has an unknown or invalid outcome").join("; ")}`
      : margins.map((m) => `${m.candidate}/${plan.instances.length} vs ${m.against} ${m.other}/${plan.instances.length}`).join("; ") + ` (success-count margin >= ${plan.criterion.min_margin} required)`;
    return { package: c.package, slot: c.slot, success_count: c.success_count, scores: c.scores, margins, promotable, unknown, reason };
  });
  // 5. At most one promotion: the best promotable candidate; ties among the best go to plan order.
  const best = verdicts.filter((v) => v.promotable).sort((a, b) => b.success_count - a.success_count)[0] || null;
  const decided = verdicts.map((v) => ({ ...v, verdict: v === best ? "promoted" : v.unknown ? "insufficient" : "rejected" }));
  for (const v of decided) append(state, { type: "verdict", plan: plan.sha256, generation: plan.generation, slot: v.slot, package: v.package.sha256, verdict: v.verdict, reason: v.reason, margins: v.margins });
  const feedbackFor = (arm) => freeze({ plan: plan.sha256, instances: idFree(arm.scores) });
  const frontier = state.frontier[family.id];
  if (best) {
    append(state, { type: "promotion", generation: plan.generation, family: family.id, package: best.package.sha256, parent: incumbent.sha256, success_count: best.success_count, incumbent_count: inc.success_count, of: plan.instances.length, plan: plan.sha256 });
    state.frontier = { ...state.frontier, [family.id]: { package: best.package, success_count: best.success_count, plan: plan.sha256, generation: plan.generation, feedback: feedbackFor(best) } };
  } else if (frontier) {
    state.frontier = { ...state.frontier, [family.id]: { ...frontier, feedback: feedbackFor(inc) } };
  } else {
    state.rootFeedback = { ...(state.rootFeedback || {}), [family.id]: feedbackFor(inc) };
  }
  return { plan, verdicts: decided, dropped, promoted: best ? best.package : null };
}

/** The loop with stop rules: a generation count, an evaluation or call budget (checked before a generation), a stall, or a refusal. */
export function run(state, { family, mutator, k = 4, controls = [], criterion, heldout = 6, generations = 10, budget = {}, stall = Infinity }) {
  let promotions = 0, sinceLast = 0, done = 0, stopped = "generations";
  for (let g = 0; g < generations; g++) {
    const cost = (Math.max(1, Number.isInteger(k) ? k : 1) + controls.length + 1) * Math.max(1, heldout);
    if (budget.evaluations !== undefined && state.evaluations + cost > budget.evaluations) { stopped = "budget"; break; }
    if (budget.calls !== undefined && state.calls + 1 > budget.calls) { stopped = "budget"; break; }
    const out = runGeneration(state, { family, mutator, k, controls, criterion, heldout });
    if (out.refused) { stopped = "refused"; break; }
    done += 1;
    if (out.promoted) { promotions += 1; sinceLast = 0; } else if (++sinceLast >= stall) { stopped = "stall"; break; }
  }
  append(state, { type: "run_stopped", family: family.id, reason: stopped, generations: done, promotions, evaluations: state.evaluations, calls: state.calls });
  return { generations: done, promotions, stopped };
}

const panelsUsed = (state) => {
  const seeds = new Set(), instances = new Set();
  for (const r of state.ledger.records) if (r.type === "plan_frozen" || r.type === "confirmation") { seeds.add(r.instance_seed); for (const i of r.instances) instances.add(i); }
  return { seeds, instances };
};

/**
 * Confirmation: score the current frontier once on a final fresh panel. The panel seed must never have been
 * used, its instances must be disjoint from every selection and confirmation panel, each frontier is confirmed
 * at most once, and an optional evaluation budget is enforced before scoring. Recorded, never fed back.
 */
export function confirm(state, { family, heldout = 6, panelSeed, budget = {} }) {
  const f = state.frontier[family.id];
  if (!f) throw new Error(`no frontier for ${family.id}`);
  if (panelSeed === undefined || panelSeed === null || panelSeed === "") throw new Error("confirm needs a panelSeed that was never used for selection");
  if (state.ledger.records.some((r) => r.type === "confirmation" && r.family === family.id && r.package === f.package.sha256 && r.selection_plan === f.plan)) throw new Error("this frontier was already confirmed and its panel already used; confirmation is once only");
  const instanceSeed = sha256(`confirm:${state.lab}:${panelSeed}:${family.id}`);
  const used = panelsUsed(state);
  if (used.seeds.has(instanceSeed)) throw new Error("that panel was already used");
  if (budget.evaluations !== undefined && state.evaluations + heldout > budget.evaluations) throw new Error("confirmation exceeds the declared evaluation budget");
  const instances = family.instances(instanceSeed, heldout);
  if (!Array.isArray(instances) || instances.length !== heldout || new Set(instances).size !== instances.length) throw new Error(`family returned ${Array.isArray(instances) ? instances.length : "no"} instances against ${heldout} declared`);
  const overlap = instances.filter((i) => used.instances.has(i));
  if (overlap.length) throw new Error(`confirmation instances were already used in selection: ${overlap.slice(0, 3).join(", ")}`);
  append(state, { type: "confirmation_declared", family: family.id, package: f.package.sha256, selection_plan: f.plan, instance_seed: instanceSeed, instances });
  const scores = instances.map((instance) => { state.evaluations += 1; const raw = family.score(f.package, instance); return validScore(raw) ? { instance, outcome: "scored", success: raw.success, progress: raw.progress, cost: raw.cost, detail: raw.detail } : { instance, outcome: "invalid" }; });
  const record = append(state, { type: "confirmation", family: family.id, package: f.package.sha256, selection_plan: f.plan, instance_seed: instanceSeed, instances, scores, success_count: scoredCount(scores), of: scores.length, outcome: scores.every((s) => s.outcome === "scored") ? "scored" : "invalid" });
  return { package: f.package.sha256, success_count: record.success_count, of: record.of, selection_count: f.success_count, outcome: record.outcome };
}

/** Passing on: the frontier package, its ancestry down to the root, and the whole ledger bound by its head. */
export function exportBundle(state, familyId) {
  const f = state.frontier[familyId];
  if (!f) throw new Error(`no frontier for ${familyId}`);
  const lineage = [], visited = new Set();
  for (let p = state.packages[f.package.sha256] || f.package; p && !visited.has(p.sha256); p = p.parent ? state.packages[p.parent] : null) {
    visited.add(p.sha256); lineage.push({ sha256: p.sha256, origin: p.origin, generation: p.generation, note: p.note, parent: p.parent });
  }
  const ledger = { records: state.ledger.records, head: state.ledger.head };
  return { package: state.packages[f.package.sha256] || f.package, family: familyId, success_count: f.success_count, plan: f.plan, lineage, ledger, ledger_sha256: ledger.head, exported_by: state.lab };
}

// A bundle is trusted only as far as its own ledger goes: the package must have been promoted there by the
// sender, and every lineage entry must be a package that ledger knows, with the parent links it recorded.
function provenanceInLedger(bundle) {
  const check = verifyLedger(bundle.ledger);
  if (!check.ok) return "bundle ledger does not verify: " + check.reason;
  if (bundle.ledger_sha256 !== check.head) return "bundle ledger does not verify: head mismatch";
  const pkg = bundle.package;
  if (!pkg || typeof pkg.source !== "string" || sha256(pkg.source) !== pkg.sha256) return "package hash does not match its source";
  const records = bundle.ledger.records;
  const known = new Map(); // sha -> { parent, origin } as the ledger recorded it
  for (const r of records) {
    if (r.type === "root_registered") known.set(r.package, { parent: null, origin: null });
    if (r.type === "candidates_proposed") for (const c of r.candidates) if (!known.has(c)) known.set(c, { parent: r.parent, origin: r.lab });
    if (r.type === "adoption" && r.verdict === "promoted" && !known.has(r.package)) known.set(r.package, { parent: r.parent ?? null, origin: r.from });
  }
  const promoted = records.some((r) => (r.type === "promotion" || (r.type === "adoption" && r.verdict === "promoted")) && r.package === pkg.sha256 && r.lab === bundle.exported_by);
  if (!promoted) return `package ${pkg.sha256.slice(0, 12)} was never promoted by ${bundle.exported_by} in its ledger`;
  if (!Array.isArray(bundle.lineage) || !bundle.lineage.length || bundle.lineage[0].sha256 !== pkg.sha256) return "lineage does not start at the package";
  for (const l of bundle.lineage) {
    const k = known.get(l.sha256);
    if (!k) return `lineage entry ${String(l.sha256).slice(0, 12)} is not in the ledger`;
    if ((l.parent ?? null) !== (k.parent ?? null)) return `lineage entry ${String(l.sha256).slice(0, 12)} has a parent the ledger did not record`;
    if (k.origin !== null && l.origin !== k.origin) return `lineage entry ${String(l.sha256).slice(0, 12)} claims an origin the ledger did not record`;
  }
  const k0 = known.get(pkg.sha256);
  if ((pkg.parent ?? null) !== (k0.parent ?? null) || (k0.origin !== null && pkg.origin !== k0.origin)) return "package provenance differs from the ledger";
  return null;
}

/** Adoption never trusts the sender's numbers: provenance is checked against the bundle's ledger, then the package is re-verified on the adopter's own panel. Every outcome is on the adopter's ledger. */
export function adopt(state, { bundle, family, controls = [], criterion, heldout = 6 }) {
  const problem = provenanceInLedger(bundle);
  const base = { type: "adoption", family: family.id, package: bundle && bundle.package ? bundle.package.sha256 : null, from: bundle ? bundle.exported_by : null, from_ledger_sha256: bundle ? bundle.ledger_sha256 : null };
  if (problem) { append(state, { ...base, verdict: "rejected", reason: problem }); return { verdict: "rejected", reason: problem }; }
  const existing = state.packages[bundle.package.sha256];
  const candidate = existing || { ...bundle.package, note: `adopted from ${bundle.exported_by}; ${bundle.package.note || ""}`.trim() };
  const out = runGeneration(state, { family, candidates: [candidate], controls, criterion, heldout });
  const v = out.refused ? { verdict: "rejected", reason: out.refused } : out.verdicts[0] || { verdict: "rejected", reason: "identical to the local incumbent or a control" };
  append(state, { ...base, parent: bundle.package.parent ?? null, lineage: bundle.lineage.map((l) => l.sha256), verdict: v.verdict, reason: v.reason });
  return { verdict: v.verdict, reason: v.reason };
}
