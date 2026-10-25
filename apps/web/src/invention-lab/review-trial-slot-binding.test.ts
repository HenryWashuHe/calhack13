// Inert retained-record substitutions only: no exporter, method, evaluator or provider.
import fs from "node:fs";
import { describe, expect, it } from "vitest";
import { canonicalSha256 } from "./digest";
import { buildLineage } from "./lineage";
import { buildMatrix } from "./matrix";
import { decodeEvaluationReceipt, decodeExecutionReceipt, decodeMetricsArtifact, decodeTrialPlan, decodeVerdict } from "./records";
import execution from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/candidate/receipt.json";
import evaluation from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/candidate-score/evaluation.json";
import plan from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/plan.json";
import verdict from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/verdict.json";

const metrics = decodeMetricsArtifact(new Uint8Array(fs.readFileSync(new URL(
  "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/candidate-score/artifacts/db11d4ff7ed356ee1ff084f185a0e7483fab71d1147892d38d2abc59d19ddc60",
  import.meta.url,
))));
function view(raw = structuredClone(execution), suppliedPlan: unknown | null = plan) {
  raw.binding_sha256 = canonicalSha256({ domain: "genesis.development_run_binding", version: 1, binding: raw.binding });
  const receipt = decodeExecutionReceipt(raw);
  const score = structuredClone(evaluation);
  score.request.execution_sha256 = receipt.receipt_sha256;
  score.request.purpose = structuredClone(raw.binding.purpose);
  score.request_sha256 = canonicalSha256({ domain: "genesis.development_evaluation_request", version: 1, record: score.request });
  const l = buildLineage({
    generations: [], executions: [receipt], evaluations: [decodeEvaluationReceipt(score)], metrics: [metrics],
    trials: suppliedPlan === null ? [] : [{ plan: decodeTrialPlan(suppliedPlan), verdict: decodeVerdict(verdict) }],
  });
  const result = [...buildMatrix(l).rows[0]!.cells.values()][0]![0]!;
  return { l, result };
}
function unbound(v: ReturnType<typeof view>) {
  expect(v.l.executions).toHaveLength(1); // Keep the record visible without borrowing the verdict.
  expect(v.l.trials[0]!.executions).toHaveLength(0);
  expect(v.result.verdict).toBeNull();
  expect(v.result.score.kind).not.toBe("counted");
  expect(v.l.problems.length).toBeGreaterThan(0);
}

describe("a supplied plan must bind the execution's full declared slot", () => {
  it("control: retained candidate keeps its count and recorded trial verdict", () => {
    const { l, result } = view();
    expect(l.problems).toEqual([]);
    expect(l.trials[0]!.executions).toHaveLength(1);
    expect(result.score).toMatchObject({ kind: "counted", successes: 1, of: 6 });
    expect(result.verdict?.verdict).toBe("accepted");
  });

  const changes: [string, (r: typeof execution) => void][] = [
    ["organization", (r) => { r.binding.org_id = "org_foreign"; }],
    ["run", (r) => { r.binding.run_id = "run_foreign"; }],
    ["attempt", (r) => { r.binding.attempt_id = "att_foreign"; }],
    ["method", (r) => { r.binding.method.sha256 = "f".repeat(64); r.actual_method = { ...r.binding.method }; }],
    ["role", (r) => { r.binding.purpose.role = "control"; }],
    ["grant", (r) => { r.binding.budget.wall_time_ms += 1; }],
    ["model settings", (r) => { r.binding.model_settings.sha256 = "f".repeat(64); }],
    ["environment", (r) => { r.binding.environment.sha256 = "f".repeat(64); r.actual_environment = { ...r.binding.environment }; }],
    ["actual environment", (r) => { r.actual_environment.sha256 = "f".repeat(64); }],
  ];
  it.each(changes)("does not lend a verdict/count to a substituted %s", (_name, change) => {
    const r = structuredClone(execution);
    change(r);
    unbound(view(r));
  });

  it.each(["absent", "unknown purpose"])("keeps %s slot evidence unsupported", (kind) => {
    const p: Record<string, unknown> = structuredClone(plan);
    if (kind === "absent") delete p.slots;
    else p.slots = [{ ...plan.slots[0], purpose: { kind: "not_supported" } }];
    const r = structuredClone(execution);
    r.binding.purpose.plan = decodeTrialPlan(p).ref;
    unbound(view(r, p));
  });
});

describe("scoring requires the evaluator frozen in the execution", () => {
  it("control: matching evaluator remains countable without a supplied plan", () => {
    expect(view(structuredClone(execution), null).result.score.kind).toBe("counted");
  });
  it.each(["trial", "ordinary"])("refuses a foreign evaluator for a %s run", (kind) => {
    const raw: Record<string, unknown> = structuredClone(execution);
    const binding = raw.binding as Record<string, unknown>;
    binding.evaluator = { ...execution.binding.evaluator, sha256: "f".repeat(64) };
    if (kind === "ordinary") binding.purpose = { kind: "ordinary" };
    const { l, result } = view(raw as typeof execution, null);
    expect(result.score.kind).not.toBe("counted");
    expect(l.problems.some((p) => /evaluator/i.test(p))).toBe(true);
  });
});
