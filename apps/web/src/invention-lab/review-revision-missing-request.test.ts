// Inert record substitutions only; no model, method, evaluator or exporter runs.
import { describe, expect, it } from "vitest";
import { buildLineage } from "./lineage";
import { decodeGenerationStatus, decodeRevisionObservation } from "./records";
import { story } from "./story";
import aStatus from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/final-status.json";
import bStatus from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/final-status.json";
import revision from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/revision-observation.json";

function view(observation: unknown) {
  const l = buildLineage({
    generations: [aStatus, bStatus].map((s) => ({ status: decodeGenerationStatus(s) })),
    revisions: [decodeRevisionObservation(observation)],
  });
  return { l, step: story(l, aStatus.org_id, bStatus.org_id)[2]! };
}

describe("revision cost needs a recorded request identity", () => {
  it("keeps measured cost when the observation and status name the same request", () => {
    const { l, step } = view(revision);
    expect(l.problems).toEqual([]);
    expect(step.text).toContain("1 model calls");
  });

  it.each(["missing", "null"] as const)("keeps a %s request unknown rather than treating it as a wildcard", (kind) => {
    const observation: Record<string, unknown> = { ...revision };
    if (kind === "missing") delete observation.request_sha256;
    else observation.request_sha256 = null;
    const { step } = view(observation);
    expect(step.state).toBe("recorded");
    expect(step.text).toContain("cost is unknown");
    expect(step.text).not.toContain("1 model calls");
  });
});
