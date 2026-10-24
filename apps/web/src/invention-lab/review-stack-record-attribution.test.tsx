// Independent #99/#100/#101 regressions. UI-only synthetic substitutions and
// retained receipt parsing; these tests execute no generated method or provider.
import { describe, expect, it } from "vitest";
import { buildLedger, totalText } from "./ledger";
import { buildLineage } from "./lineage";
import { decodeGenerationStatus } from "./records";
import status from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/final-status.json";

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
