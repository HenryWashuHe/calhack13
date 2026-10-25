// Independent #99/#100/#101 regressions. UI-only synthetic substitutions and
// retained receipt parsing; these tests execute no generated method or provider.
import { createHash } from "node:crypto";
import { describe, expect, it } from "vitest";
import { buildLedger, totalText } from "./ledger";
import { buildLineage } from "./lineage";
import { decodeEvaluationReceipt, decodeExecutionReceipt, decodeGenerationStatus } from "./records";
import status from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/final-status.json";
import execution from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/candidate/receipt.json";
import evaluation from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/candidate-score/evaluation.json";

const request = (org: string) => (org === "org_a" ? "a" : "b").repeat(64);
function generation(org: string, operation: string, calls: number, pkg = status.package) {
  return decodeGenerationStatus({
    ...structuredClone(status),
    org_id: org,
    operation_id: operation,
    request_sha256: request(org),
    package: pkg,
    usage: { model_calls: { kind: "measured", value: calls } },
  });
}

// The retained records contain integers only. Match the existing sorted-key
// JSON domains to establish that the substituted run has a different identity.
function canonical(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") {
    const object = value as Record<string, unknown>;
    return `{${Object.keys(object).sort().map((key) => `${JSON.stringify(key)}:${canonical(object[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}
const hash = (value: unknown) => createHash("sha256").update(canonical(value)).digest("hex");
const executionHash = (record: unknown) => hash({ domain: "genesis.development_execution_receipt", version: 1, record });

describe("PR99: exact arithmetic totals", () => {
  function total(a: number, b: number) {
    return totalText(buildLedger(buildLineage({ generations: [
      { status: generation("org_a", "one", a) },
      { status: generation("org_a", "two", b) },
    ] }).attempts).totals.model_calls);
  }

  it("preserves the ordinary measured sum", () => {
    expect(total(10, 2)).toBe("12");
  });

  it("does not silently round a total of individually safe measured counts", () => {
    expect(total(Number.MAX_SAFE_INTEGER, 2)).toBe("9007199254740993");
  });
});

describe("PR100: scoring must name the actual execution", () => {
  it("attaches the retained evaluation to its exact original execution", () => {
    expect(evaluation.request.execution_sha256).toBe(executionHash(execution));
    const lineage = buildLineage({
      generations: [],
      executions: [decodeExecutionReceipt(execution)],
      evaluations: [decodeEvaluationReceipt(evaluation)],
    });
    expect(lineage.executions[0]!.evaluations).toHaveLength(1);
  });

  it("does not attach another run's evaluation merely because output and purpose match", () => {
    const other = structuredClone(execution);
    other.binding.org_id = "org_unrelated";
    other.binding.run_id = "different-run";
    other.binding.attempt_id = "different-attempt";
    other.binding_sha256 = hash({ domain: "genesis.development_run_binding", version: 1, binding: other.binding });
    expect(evaluation.request.execution_sha256).not.toBe(executionHash(other));
    const lineage = buildLineage({
      generations: [],
      executions: [decodeExecutionReceipt(other)],
      evaluations: [decodeEvaluationReceipt(evaluation)],
    });
    expect(lineage.executions[0]!.receipt.org_id).toBe("org_unrelated");
    expect(lineage.executions[0]!.evaluations).toHaveLength(0);
  });
});
