// Eric's Claude: the only words this view may use (docs/invention-lab/ui.md).
// Every label comes from a recorded value; anything else is "unrecognized".
// There is deliberately no "improved" label: that needs a predeclared
// comparison record, and no such record type exists yet.

export type Tone = "ok" | "warn" | "bad" | "neutral";
export interface Label {
  text: string;
  tone: Tone;
  /** What the label does NOT establish, shown alongside it. */
  limit: string;
}

function unrecognized(raw: string): Label {
  return { text: `unrecognized: ${raw}`, tone: "neutral", limit: "value not in this view's vocabulary; shown verbatim" };
}

const PHASE: Record<string, Label> = {
  prepared: { text: "declared, not submitted", tone: "neutral", limit: "no provider request recorded" },
  requesting: { text: "outcome unknown — do not replay", tone: "warn", limit: "a request may have been sent" },
  generated: { text: "generated source", tone: "ok", limit: "not executed, accepted or shown to improve anything" },
  rejected_source: { text: "source rejected", tone: "bad", limit: "the call still counts toward cost" },
  failed: { text: "generation failed", tone: "bad", limit: "the call still counts toward cost" },
  needs_reconciliation: { text: "outcome unknown — do not replay", tone: "warn", limit: "operator must inspect retained state" },
};
export function phaseLabel(phase: string): Label {
  return PHASE[phase] ?? unrecognized(phase);
}

const ADOPTION: Record<string, Label> = {
  adopted: { text: "adopted", tone: "ok", limit: "this organization's own checks only; not a recommendation or improvement" },
  not_applicable: { text: "not applicable here", tone: "warn", limit: "rejected by this organization's local checks" },
  insufficient_evidence: { text: "insufficient evidence", tone: "warn", limit: "no verdict could be reached" },
  unknown: { text: "unknown — retained, not concluded", tone: "warn", limit: "not a rejection and not an empty result" },
};
export function adoptionLabel(outcome: string): Label {
  return ADOPTION[outcome] ?? unrecognized(outcome);
}

// The first rejected check named by docs/invention-lab/contracts/generation.md.
const REJECTION: Record<string, string> = {
  response_unavailable: "no response body was available",
  invalid_json: "response was not valid JSON",
  invalid_envelope: "response envelope was malformed",
  model_mismatch: "responding model differed from the requested one",
  stop_reason: "stop reason was not accepted",
  stop_sequence: "a stop sequence ended the response",
  output_limit: "output exceeded the declared limit",
  content_shape: "response content shape was not accepted",
  content_count: "unexpected number of content blocks",
  content_type: "unexpected content block type",
  text_limit: "text exceeded the declared limit",
  citations: "response carried citations",
};
const STOP = new Set(["end_turn", "max_tokens", "stop_sequence", "tool_use", "other"]);
export function diagnosticLabel(rejection: string, stopReason: string | null): Label {
  const what = REJECTION[rejection];
  if (!what) return unrecognized(rejection);
  const stop =
    stopReason === null ? "no stop reason retained" : STOP.has(stopReason) ? `stop reason ${stopReason}` : `unrecognized stop: ${stopReason}`;
  return {
    text: `rejected at ${rejection}: ${what} (${stop})`,
    tone: "bad",
    limit: "first failed check only; raw content was not retained, so the cause is not reconstructable",
  };
}

export function revisionLabel(sourceChanged: boolean): Label {
  return sourceChanged
    ? { text: "source changed (bytes)", tone: "neutral", limit: "different bytes only; behavior may be identical" }
    : { text: "source unchanged", tone: "neutral", limit: "renaming or reversioning does not change source" };
}

export type Tier = "loopback_fixture" | "unobserved" | "operator_observed_cloud" | "unrecognized";
export const TIER_TEXT: Record<Tier, string> = {
  loopback_fixture: "loopback fixture",
  unobserved: "unobserved",
  operator_observed_cloud: "operator-observed cloud (hashes are local consistency, not provider authentication)",
  unrecognized: "unrecognized transport",
};
/** Tier from the recorded transport field only, never from file location. */
export function tierOf(transport: string | null): Tier {
  if (transport === null || transport === "unobserved") return "unobserved";
  if (transport === "loopback_fixture") return "loopback_fixture";
  if (transport === "anthropic_https") return "operator_observed_cloud";
  return "unrecognized";
}
