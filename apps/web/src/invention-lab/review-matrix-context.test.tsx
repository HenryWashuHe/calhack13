// Independent #148 review: inert retained JSON and synthetic substitutions only.
// No method, evaluator, provider, browser or exporter execution.
import fs from "node:fs";
import { describe, expect, it } from "vitest";
import { canonicalSha256 } from "./digest";
import { buildLineage } from "./lineage";
import { buildMatrix } from "./matrix";
import { decodeEvaluationReceipt, decodeExecutionReceipt, decodeMetricsArtifact, decodeTrialPlan, refKey } from "./records";
import execution from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/candidate/receipt.json";
import evaluation from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/candidate-score/evaluation.json";
import plan from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/plan.json";

const metrics = decodeMetricsArtifact(new Uint8Array(fs.readFileSync(new URL(
  "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/candidate-score/artifacts/db11d4ff7ed356ee1ff084f185a0e7483fab71d1147892d38d2abc59d19ddc60",
  import.meta.url,
))));
function lineage(ev = evaluation) {
  return buildLineage({
    generations: [],
    executions: [decodeExecutionReceipt(execution)],
    evaluations: [decodeEvaluationReceipt(ev)],
    metrics: [metrics],
    trials: [{ plan: decodeTrialPlan(plan), verdict: null }],
  });
}

describe("PR148: exact columns and declared scoring context", () => {
  it("counts the unchanged retained score under its exact supplied plan", () => {
    const l = lineage();
    expect(l.problems).toEqual([]);
    expect(l.executions[0]!.score).toMatchObject({ kind: "counted", successes: 1, of: 6 });
  });

  it("keeps task references with equal display labels but different domains in separate columns", () => {
    const other = structuredClone(execution);
    other.binding.public_task.domain = "another-task-domain";
    other.binding.run_id = "different-run";
    const first = decodeExecutionReceipt(execution);
    const second = decodeExecutionReceipt(other);
    expect(refKey(first.public_task)).not.toBe(refKey(second.public_task));
    const l = buildLineage({ generations: [], executions: [first, second] });
    const matrix = buildMatrix(l);
    expect(matrix.families).toHaveLength(2);
    expect(new Set([...matrix.familyRefs.values()].map(refKey)).size).toBe(2);
  });

  it("does not count a requested and observed scoring rule that disagree with the supplied plan", () => {
    const changed = structuredClone(evaluation);
    changed.request.context.scoring_rule.sha256 = "f".repeat(64);
    changed.observed_context.scoring_rule.sha256 = "f".repeat(64);
    changed.request_sha256 = canonicalSha256({ domain: "genesis.development_evaluation_request", version: 1, record: changed.request });
    const l = lineage(changed);
    expect(l.executions[0]!.score.kind).not.toBe("counted");
    expect(l.problems.length).toBeGreaterThan(0);
  });
});
