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

/** Own-property lookup. Record-controlled strings such as "constructor",
 * "toString" or "__proto__" must not resolve to inherited Object members. */
export function own<T>(table: Readonly<Record<string, T>>, key: string): T | undefined {
  return Object.hasOwn(table, key) ? table[key] : undefined;
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
  return own(PHASE, phase) ?? unrecognized(phase);
}

const ADOPTION: Record<string, Label> = {
  adopted: { text: "adopted", tone: "ok", limit: "this organization's own checks only; not a recommendation or improvement" },
  not_applicable: { text: "not applicable here", tone: "warn", limit: "rejected by this organization's local checks" },
  insufficient_evidence: { text: "insufficient evidence", tone: "warn", limit: "no verdict could be reached" },
  unknown: { text: "unknown — retained, not concluded", tone: "warn", limit: "not a rejection and not an empty result" },
};
export function adoptionLabel(outcome: string): Label {
  return own(ADOPTION, outcome) ?? unrecognized(outcome);
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
