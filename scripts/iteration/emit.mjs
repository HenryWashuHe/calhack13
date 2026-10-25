// Evidence-shape emitter: render an engine plan and verdicts in the shapes already retained under
// docs/invention-lab/evidence/*/{plan,criterion,verdict}.json, so the reference never bypasses them.
// Every ExactContentRef comes from the adapter; this file invents no identity, hash or grant.

const HEX64 = /^[0-9a-f]{64}$/;
export const isExactRef = (r) => !!r && typeof r === "object" && ["domain", "schema", "id", "version"].every((k) => typeof r[k] === "string" && r[k].length > 0) && HEX64.test(String(r.sha256));
const need = (r, name) => { if (!isExactRef(r)) throw new Error(`refs.${name} must be an ExactContentRef {domain, schema, id, version, sha256}`); return r; };
const GRANT_KEYS = ["model_calls", "input_tokens", "output_tokens", "wall_time_ms", "simulation_steps"];
const grant = (g, name) => { if (!g || !GRANT_KEYS.every((k) => Number.isInteger(g[k]) && g[k] >= 0)) throw new Error(`grants.${name} must declare ${GRANT_KEYS.join(", ")}`); return Object.fromEntries(GRANT_KEYS.map((k) => [k, g[k]])); };

/**
 * development-trial-plan/1 from a frozen engine plan. `refs` supplies the criterion, context, model
 * settings and environment refs plus `instance(id)` and `method(pkg)` functions; `grants` the two grants.
 * The incumbent and controls are control arms; candidates are candidate arms; order is the plan's.
 */
export function toTrialPlan(plan, { refs, grants, org_id, id, version = "1", packages = {} }) {
  if (!plan || !Array.isArray(plan.arms)) throw new Error("a frozen engine plan is required");
  const context = Object.fromEntries(["public_task", "evaluator", "environment", "generator", "instance_set", "design_schema", "metric_schema", "scoring_rule"].map((k) => [k, need(refs[k], k)]));
  const slots = plan.arms.map((arm) => {
    const pkg = packages[arm.package] || { sha256: arm.package, note: arm.slot };
    const method = need(refs.method(pkg), `method(${arm.slot})`);
    if (method.sha256 !== arm.package) throw new Error(`refs.method must keep the package hash for ${arm.slot}`);
    const role = arm.slot.startsWith("candidate") ? "candidate" : "control";
    const key = arm.slot.replace(/[^a-z0-9]+/gi, "-");
    return { slot_id: arm.slot, scoring_request_id: `score-${key}`, org_id, run_id: `run-${key}`, attempt_id: `att_${key}`, method, dependencies: [], purpose: { kind: "acceptance", role } };
  });
  return {
    id, version,
    criterion: need(refs.criterion, "criterion"),
    development_instances: plan.instances.map((i) => need(refs.instance(i), `instance(${i})`)),
    scoring_instances: { kind: "development" },
    context,
    model_settings: need(refs.model_settings, "model_settings"),
    development_environment: need(refs.development_environment, "development_environment"),
    development_grant: grant(grants && grants.development, "development"),
    scoring_grant: grant(grants && grants.scoring, "scoring"),
    slots,
  };
}

/** criterion.json: the instances the comparison is scored on and the declared rule. */
export function toCriterion(plan) {
  return { instances: [...plan.instances], min_margin: plan.criterion.min_margin, rule: plan.criterion.rule };
}

/** verdict.json: the retained vocabulary (accepted/rejected) and a reason that states the counts and the margin. */
export function toVerdict(verdict, plan) {
  const n = plan.instances.length;
  const worst = [...verdict.margins].sort((a, b) => a.margin - b.margin)[0];
  const ok = verdict.verdict === "promoted";
  const reason = ok
    ? `candidate ${verdict.success_count}/${n} exceeds every arm by ${plan.criterion.min_margin} (closest: ${worst.against} ${worst.other}/${n})`
    : `candidate ${verdict.success_count}/${n} does not exceed ${worst.against} ${worst.other}/${n} by ${plan.criterion.min_margin}`;
  return { reason, verdict: ok ? "accepted" : "rejected" };
}
