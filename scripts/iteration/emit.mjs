// Evidence-shape emitter: render an engine generation as retained-shape documents without bypassing the
// retained contracts. One engine plan with many arms becomes one acceptance TrialPlan per
// (candidate, control-arm) pairing, each with exactly one candidate and one control slot and
// trial-qualified identities. Method identities and dependencies come from the adapter as the real
// package references; the engine's raw source hash travels separately under its own schema. Verdicts are
// recomputed per pairing from the frozen counts (accepted / rejected / insufficient), independently of
// the engine's one-frontier selection. Pure functions; nothing is written or executed here.
import { sha256, canon } from "./engine.mjs";

const HEX64 = /^[0-9a-f]{64}$/;
export const isExactRef = (r) => !!r && typeof r === "object" && ["domain", "schema", "id", "version"].every((k) => typeof r[k] === "string" && r[k].length > 0) && HEX64.test(String(r.sha256));
const need = (r, name) => { if (!isExactRef(r)) throw new Error(`refs.${name} must be an ExactContentRef {domain, schema, id, version, sha256}`); return r; };
const GRANT_KEYS = ["model_calls", "input_tokens", "output_tokens", "wall_time_ms", "simulation_steps"];
const grant = (g, name) => { if (!g || !GRANT_KEYS.every((k) => Number.isInteger(g[k]) && g[k] >= 0)) throw new Error(`grants.${name} must declare ${GRANT_KEYS.join(", ")}`); return Object.fromEntries(GRANT_KEYS.map((k) => [k, g[k]])); };
const slug = (s) => String(s).replace(/[^a-z0-9]+/gi, "-").replace(/^-|-$/g, "").toLowerCase();

/**
 * development-trial-plan/1 documents for a frozen engine plan: one per (candidate arm, control arm) pairing.
 * `refs` supplies criterion, context, model settings and environment refs plus `instance(id)`, `method(pkg)`
 * and optional `dependencies(pkg)`; `grants` the two grants; `packages` resolves arm hashes to packages.
 * Returns { plans, sources }: `sources` maps each plan's slot to the engine's raw source identity.
 */
export function toTrialPlans(plan, { refs, grants, org_id, id, version = "1", packages = {} }) {
  if (!plan || !Array.isArray(plan.arms) || !Array.isArray(plan.instances)) throw new Error("a frozen engine plan is required");
  if (typeof org_id !== "string" || !org_id || typeof id !== "string" || !id) throw new Error("org_id and id are required");
  const context = Object.fromEntries(["public_task", "evaluator", "environment", "generator", "instance_set", "design_schema", "metric_schema", "scoring_rule"].map((k) => [k, need(refs[k], k)]));
  const common = {
    criterion: need(refs.criterion, "criterion"),
    development_instances: plan.instances.map((i) => need(refs.instance(i), `instance(${i})`)),
    scoring_instances: { kind: "development" },
    context,
    model_settings: need(refs.model_settings, "model_settings"),
    development_environment: need(refs.development_environment, "development_environment"),
    development_grant: grant(grants && grants.development, "development"),
    scoring_grant: grant(grants && grants.scoring, "scoring"),
  };
  const candidates = plan.arms.filter((a) => a.slot.startsWith("candidate"));
  const controls = plan.arms.filter((a) => !a.slot.startsWith("candidate"));
  if (!candidates.length || !controls.length) throw new Error("a pairing needs at least one candidate arm and one control arm");
  const plans = [], sources = {};
  for (const cand of candidates) for (const ctrl of controls) {
    const pairId = `${id}-g${plan.generation}-${slug(cand.slot)}-vs-${slug(ctrl.slot)}`;
    const qualifier = sha256(canon({ engine_plan: plan.sha256, pairing: pairId })).slice(0, 16); // distinct per generation and pairing
    const slot = (arm, role) => {
      const pkg = packages[arm.package] || { sha256: arm.package };
      const method = need(refs.method(pkg), `method(${arm.slot})`); // the real package reference, never the raw source hash
      const deps = refs.dependencies ? refs.dependencies(pkg) : [];
      if (!Array.isArray(deps) || !deps.every(isExactRef)) throw new Error(`refs.dependencies(${arm.slot}) must return ExactContentRefs`);
      return { slot_id: role, scoring_request_id: `score-${role}-${qualifier}`, org_id, run_id: `run-${role}-${qualifier}`, attempt_id: `att_${role}_${qualifier}`, method, dependencies: deps, purpose: { kind: "acceptance", role } };
    };
    plans.push({ id: pairId, version, ...common, slots: [slot(cand, "candidate"), slot(ctrl, "control")] });
    sources[pairId] = { candidate: { schema: "iteration-source/1", sha256: cand.package, arm: cand.slot }, control: { schema: "iteration-source/1", sha256: ctrl.package, arm: ctrl.slot } };
  }
  return { plans, sources };
}

/** criterion.json: the instances the comparison is scored on and the declared rule. */
export function toCriterion(plan) {
  return { instances: [...plan.instances], min_margin: plan.criterion.min_margin, rule: plan.criterion.rule };
}

/**
 * verdict.json per pairing, recomputed from the frozen counts: accepted when the candidate exceeds that
 * control by the declared margin, rejected when both counts are known and it does not, insufficient when
 * either outcome is unknown. Independent of the engine's single-frontier selection.
 */
export function toVerdicts(verdict, plan) {
  const of = plan.instances.length;
  return verdict.margins.map((m) => {
    const known = m.known !== false && !verdict.unknown;
    const accepted = known && m.margin >= plan.criterion.min_margin;
    const reason = !known ? `insufficient: ${m.against} or the candidate has an unknown outcome`
      : accepted ? `candidate ${m.candidate}/${of} exceeds ${m.against} ${m.other}/${of} by ${plan.criterion.min_margin}`
      : `candidate ${m.candidate}/${of} does not exceed ${m.against} ${m.other}/${of} by ${plan.criterion.min_margin}`;
    return { against: m.against, document: { candidate: m.candidate, control: m.other, of, reason, verdict: !known ? "insufficient" : accepted ? "accepted" : "rejected" } };
  });
}
