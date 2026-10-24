// Eric's Claude: decoders and labels against the actual retained first live
// attempt (imported from the committed evidence, not copied) plus synthetic
// records for cases that evidence doesn't cover.
import { describe, expect, it } from "vitest";
import { adoptionLabel, phaseLabel, revisionLabel, tierOf } from "./claims";
import { DecodeError, decodeCheckpointOutcome, decodeGenerationStatus, decodeRevisionObservation } from "./records";
import finalStatus from "../../../../docs/invention-lab/evidence/live-method-20261024/final-status.json";
import generationCheckpoint from "../../../../docs/invention-lab/evidence/live-method-20261024/generation.json";

const status = () => structuredClone(finalStatus) as Record<string, unknown>;

describe("first live attempt (retained evidence)", () => {
  it("decodes as an operator-observed cloud generation failure with measured cost", () => {
    const s = decodeGenerationStatus(status());
    expect(phaseLabel(s.phase).text).toBe("generation failed");
    expect(tierOf(s.transport)).toBe("operator_observed_cloud");
    expect(s.settings.sampling).toBe("provider_default");
    expect(s.package).toBeNull();
    expect(s.usage).toMatchObject({
      model_calls: { kind: "measured", value: 1 },
      input_tokens: { kind: "measured", value: 3971 },
      output_tokens: { kind: "measured", value: 8192 },
      wall_time_ms: { kind: "measured", value: 62131 },
      simulation_steps: { kind: "not_recorded" },
    });
  });

  it("reads only the enum outcome from the checkpoint", () => {
    const c = decodeCheckpointOutcome(structuredClone(generationCheckpoint));
    expect(c.operation_id).toBe("live-a-method-20261024t152819z-2a3e0b5a");
    expect(c.outcome).toEqual({ kind: "failed", reason: "protocol" });
    expect(JSON.stringify(c)).not.toContain("Generate one complete");
  });
});

describe("decoders", () => {
  it("never copy returned model text from a checkpoint", () => {
    const c = decodeCheckpointOutcome({
      plan: { operation_id: "op" },
      phase: "generated",
      request_sha256: "r",
      completion: { outcome: { kind: "returned", text: "SECRET SOURCE" } },
    });
    expect(c.outcome).toEqual({ kind: "returned", reason: null });
    expect(JSON.stringify(c)).not.toContain("SECRET");
  });

  it("keep unknown usage and unexpected enum values visible", () => {
    const s = decodeGenerationStatus({
      ...status(),
      phase: "teleported",
      transport: "carrier_pigeon",
      usage: { model_calls: { kind: "unknown", reason: "interrupted" }, input_tokens: { kind: "estimated", value: 3 } },
    });
    expect(phaseLabel(s.phase).text).toBe("unrecognized: teleported");
    expect(tierOf(s.transport)).toBe("unrecognized");
    expect(s.usage!.model_calls).toEqual({ kind: "unknown", reason: "interrupted" });
    expect(s.usage!.input_tokens).toEqual({ kind: "unrecognized", raw: "estimated" });
    expect(s.usage!.output_tokens).toEqual({ kind: "not_recorded" });
  });

  it("reject malformed records instead of guessing", () => {
    const bad = { ...status(), usage: { input_tokens: { kind: "measured", value: 1.5 } } };
    expect(() => decodeGenerationStatus(bad)).toThrow(DecodeError);
    expect(() => decodeRevisionObservation({ schema: "other/1" })).toThrow(DecodeError);
  });
});

it("has no label that claims improvement", () => {
  const words = [
    ...["adopted", "not_applicable", "insufficient_evidence", "unknown"].map(adoptionLabel),
    revisionLabel(true),
    revisionLabel(false),
    ...["prepared", "requesting", "generated", "rejected_source", "failed", "needs_reconciliation"].map(phaseLabel),
  ].map((w) => w.text);
  expect(words.join(" ")).not.toMatch(/improv|better|success/i);
});

it("labels inherited Object member names as unrecognized, not as their prototype values", () => {
  for (const raw of ["constructor", "toString", "__proto__", "hasOwnProperty", "valueOf"]) {
    for (const label of [phaseLabel(raw), adoptionLabel(raw)]) {
      expect(label).toEqual({ text: `unrecognized: ${raw}`, tone: "neutral", limit: "value not in this view's vocabulary; shown verbatim" });
    }
  }
  expect(phaseLabel("failed").text).toBe("generation failed");
  expect(adoptionLabel("adopted").text).toBe("adopted");
});
