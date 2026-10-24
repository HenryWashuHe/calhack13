// Eric's Claude: diagnostics and the attempt ledger over both retained live
// attempts (imported from the committed evidence), plus synthetic gaps.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AttemptLedger } from "./AttemptLedger";
import { diagnosticLabel } from "./claims";
import { buildLedger, totalText } from "./ledger";
import { buildLineage } from "./lineage";
import { decodeCheckpointOutcome, decodeGenerationStatus } from "./records";
import firstStatus from "../../../../docs/invention-lab/evidence/live-method-20261024/final-status.json";
import firstCheckpoint from "../../../../docs/invention-lab/evidence/live-method-20261024/generation.json";
import secondStatus from "../../../../docs/invention-lab/evidence/live-method-20261024-diagnostic/final-status.json";
import secondCheckpoint from "../../../../docs/invention-lab/evidence/live-method-20261024-diagnostic/generation.json";

const live = () =>
  buildLineage({
    generations: [
      { status: decodeGenerationStatus(structuredClone(firstStatus)), checkpoint: decodeCheckpointOutcome(structuredClone(firstCheckpoint)) },
      { status: decodeGenerationStatus(structuredClone(secondStatus)), checkpoint: decodeCheckpointOutcome(structuredClone(secondCheckpoint)) },
    ],
  });

describe("diagnostics", () => {
  it("decode the second attempt's retained diagnostic and leave the first without one", () => {
    const [first, second] = live().attempts;
    expect(first!.status.diagnostic).toBeNull();
    expect(second!.status.diagnostic).toEqual({ rejection: "content_shape", stop_reason: "end_turn" });
    expect(second!.status.unrecognized_fields).toEqual([]);
    expect(diagnosticLabel("content_shape", "end_turn").text).toBe(
      "rejected at content_shape: response content shape was not accepted (stop reason end_turn)",
    );
  });

  it("show unexpected diagnostic values and unknown status fields instead of dropping them", () => {
    expect(diagnosticLabel("vibes", null).text).toBe("unrecognized: vibes");
    expect(diagnosticLabel("output_limit", "pause_turn").text).toContain("unrecognized stop: pause_turn");
    expect(diagnosticLabel("output_limit", null).text).toContain("no stop reason retained");
    const s = decodeGenerationStatus({ ...structuredClone(secondStatus), signature: "x", aardvark: 1 });
    expect(s.unrecognized_fields).toEqual(["aardvark", "signature"]);
  });
});

describe("ledger", () => {
  it("totals both live attempts exactly as the evidence README does", () => {
    const l = buildLedger(live().attempts);
    expect(l.attempts).toBe(2);
    expect(l.byPhase).toEqual({ failed: 2 });
    expect(l.packages).toBe(0);
    expect(l.tiers).toEqual(["operator_observed_cloud"]);
    expect(totalText(l.totals.model_calls)).toBe("2");
    expect(totalText(l.totals.input_tokens)).toBe("7999");
    expect(totalText(l.totals.output_tokens)).toBe("13084");
    expect(totalText(l.totals.wall_time_ms)).toBe("98921");
    expect(totalText(l.totals.simulation_steps)).toBe("not recorded");
  });

  it("turns any unmeasured attempt into a lower bound, never a zero", () => {
    const unknown = decodeGenerationStatus({
      ...structuredClone(secondStatus),
      operation_id: "op-unknown",
      phase: "needs_reconciliation",
      usage: { model_calls: { kind: "unknown", reason: "interrupted" }, input_tokens: { kind: "measured", value: 10 } },
    });
    const l = buildLedger(buildLineage({ generations: [...live().attempts.map((a) => ({ status: a.status })), { status: unknown }] }).attempts);
    expect(totalText(l.totals.model_calls)).toBe("≥ 2 (1 of 3 not measured)");
    expect(totalText(l.totals.input_tokens)).toBe("8009");
    expect(totalText(l.totals.output_tokens)).toBe("≥ 13084 (1 of 3 not measured)");
  });

  it("renders the totals, both diagnostics states and the comparison disclaimer", () => {
    const html = renderToStaticMarkup(<AttemptLedger attempts={live().attempts} />);
    const text = html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");
    expect(text).toContain("2 attempts · 2 generation failed · 0 packages");
    expect(text).toMatch(/input_tokens 7999 .*output_tokens 13084/);
    expect(text).toContain("rejected at content_shape");
    expect(text).toContain("Not a predeclared combined budget, a cost per success or a comparison.");
    // Only the second attempt retained a diagnostic.
    expect((html.match(/class="lab-diagnostic"/g) ?? []).length).toBe(1);
  });

  it("says so when there is nothing to total", () => {
    expect(renderToStaticMarkup(<AttemptLedger attempts={[]} />)).toContain("No generation attempts recorded");
  });
});
