// Independent PR93/94 review regressions. These are synthetic UI records,
// not execution, provider, adoption or improvement evidence.
import { describe, expect, it } from "vitest";
import { adoptionLabel, phaseLabel } from "./claims";
import { buildLineage } from "./lineage";
import {
  decodeGenerationStatus,
  decodeRevisionObservation,
  type ExactRef,
} from "./records";

function method(id: string, digit: string): ExactRef {
  return {
    domain: "genesis.development_method_source",
    schema: "development-method-source/1",
    id,
    version: "1.0.0",
    sha256: digit.repeat(64),
  };
}

function generation(org: string, pkg: ExactRef) {
  return decodeGenerationStatus({
    org_id: org,
    operation_id: "shared-operation",
    phase: "generated",
    settings: {
      model: "fixture",
      max_tokens: 100,
      timeout_ms: 1000,
      sampling: "provider_default",
    },
    plan_sha256: "1".repeat(64),
    request_sha256: (org === "org_a" ? "2" : "3").repeat(64),
    wire_sha256: "4".repeat(64),
    transport: "loopback_fixture",
    actual_model: "fixture",
    usage: { model_calls: { kind: "measured", value: 1 } },
    package: pkg,
    authority: "Synthetic record for UI identity regression only.",
  });
}

function revision(org: string, child: ExactRef) {
  return decodeRevisionObservation({
    schema: "genesis-method-revision-observation/1",
    org_id: org,
    operation_id: "shared-operation",
    parent: method("parent", "0"),
    child,
    transport: "loopback_fixture",
    source_changed: true,
  });
}

describe("PR93: all unrecognized enum strings have visible labels", () => {
  it("does not resolve prototype property names as labels", () => {
    for (const label of [phaseLabel, adoptionLabel]) {
      for (const raw of ["constructor", "toString", "__proto__"]) {
        expect.soft(label(raw), `${label.name}(${raw})`).toMatchObject({
          text: `unrecognized: ${raw}`,
          tone: "neutral",
          limit: expect.any(String),
        });
      }
    }
  });

  it("retains ordinary known and unrecognized labels", () => {
    expect(phaseLabel("failed").text).toBe("generation failed");
    expect(adoptionLabel("unknown").text).toBe("unknown — retained, not concluded");
    expect(phaseLabel("unlisted-phase").text).toBe("unrecognized: unlisted-phase");
    expect(adoptionLabel("unlisted-outcome").text).toBe("unrecognized: unlisted-outcome");
  });
});

describe("PR94: generation identity includes organization and operation", () => {
  const a = method("method-a", "a");
  const b = method("method-b", "b");

  it("does not report contradictions for two organizations reusing an operation ID", () => {
    const lineage = buildLineage({
      generations: [
        { status: generation("org_a", a) },
        { status: generation("org_b", b) },
      ],
      revisions: [revision("org_b", b)],
    });
    expect(lineage.attempts).toHaveLength(2);
    expect(lineage.problems).toEqual([]);
  });

  it("does not let another organization's matching package hide the real contradiction", () => {
    const lineage = buildLineage({
      generations: [
        { status: generation("org_a", a) },
        { status: generation("org_b", b) },
      ],
      revisions: [revision("org_b", a)],
    });
    expect(lineage.problems.some((p) => p.includes("revision child differs"))).toBe(true);
  });

  it("still reports repeated attempts within the same organization", () => {
    const lineage = buildLineage({
      generations: [
        { status: generation("org_b", b) },
        { status: generation("org_b", b) },
      ],
    });
    expect(lineage.problems.some((p) => p.includes("appears more than once"))).toBe(true);
  });

  it("still reports a real single-organization revision contradiction", () => {
    const lineage = buildLineage({
      generations: [{ status: generation("org_b", b) }],
      revisions: [revision("org_b", a)],
    });
    expect(lineage.problems.some((p) => p.includes("revision child differs"))).toBe(true);
  });
});
