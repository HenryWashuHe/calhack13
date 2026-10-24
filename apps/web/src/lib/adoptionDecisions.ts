// Henry's Claude: the adoption decision trail for one gap. Need → candidates
// the registry offered → why each was or wasn't used → this organization's
// local acceptance → what the adoption concluded. Everything shown comes from
// the server: `outcome` on `GET /capability-gaps/{id}/adoption` (#128) and
// `GET /capability-adoptions/{id}/candidates` (#116). Nothing is inferred:
// no lineage beyond the recorded publisher, no "improved" claim, and time and
// cost read "not measured" because the backend doesn't record them yet.

import { useEffect, useRef, useState } from "react";
import type { Api } from "./api";

export type OutcomeKind =
  | "pending"
  | "adopted"
  | "not_applicable"
  | "insufficient_evidence"
  | "no_candidate"
  | "unknown"
  | "stopped";

export type CandidateDecisionKind =
  | "selected"
  | "adopted"
  | "not_applicable"
  | "own_publication"
  | "missing_scope"
  | "invalid_manifest"
  | "unusable_state"
  | "manifest_unavailable"
  | "invalid_response"
  | "installation_conflict"
  | "not_evaluated";

/** Mirrors the server's `OutcomeView`. */
export interface OutcomeView {
  outcome: OutcomeKind;
  reason: string;
  considered: number;
  decisions: Record<string, number>;
  candidate: {
    capability_id: string;
    version: string;
    publisher: string;
    decision: CandidateDecisionKind;
    acceptance_ref: string | null;
  } | null;
}

/** Mirrors the server's `CandidateDecision`. Score orders only. */
export interface CandidateDecision {
  rank: number;
  score: number | null;
  capability_id: string;
  version: string;
  publisher: string;
  source_digest: string;
  applicability_conditions: string[];
  required_permissions: string[];
  decision: CandidateDecisionKind;
  reason: string;
  missing_scopes: string[];
  acceptance_ref: string | null;
  decided_at: string;
}

type Tone = "ok" | "warn" | "bad" | "neutral" | "accent";

export const OUTCOME: Record<OutcomeKind, { label: string; tone: Tone; meaning: string }> = {
  pending: { label: "pending", tone: "neutral", meaning: "adoption is still working" },
  adopted: { label: "adopted", tone: "ok", meaning: "passed this organization's own checks and was installed" },
  not_applicable: {
    label: "not applicable",
    tone: "warn",
    meaning: "tested here and rejected by this organization's own checks",
  },
  insufficient_evidence: {
    label: "insufficient evidence",
    tone: "warn",
    meaning: "offered, but no verdict could be reached",
  },
  no_candidate: { label: "no candidate", tone: "neutral", meaning: "nothing eligible was published" },
  unknown: {
    label: "unknown",
    tone: "bad",
    meaning: "the registry or a check couldn't be read; not treated as empty",
  },
  stopped: { label: "stopped", tone: "neutral", meaning: "stopped for authority or configuration; no verdict" },
};

export const DECISION: Record<CandidateDecisionKind, { label: string; tone: Tone }> = {
  selected: { label: "selected for checks", tone: "accent" },
  adopted: { label: "adopted", tone: "ok" },
  not_applicable: { label: "rejected by local checks", tone: "warn" },
  own_publication: { label: "own publication", tone: "neutral" },
  missing_scope: { label: "missing permission", tone: "neutral" },
  invalid_manifest: { label: "invalid manifest", tone: "warn" },
  unusable_state: { label: "not installable", tone: "warn" },
  manifest_unavailable: { label: "manifest unreadable", tone: "bad" },
  invalid_response: { label: "invalid registry answer", tone: "warn" },
  installation_conflict: { label: "already installed", tone: "neutral" },
  not_evaluated: { label: "not evaluated", tone: "neutral" },
};

/**
 * What the evidence on screen does and doesn't establish. The backend records
 * local acceptance, but not whether the provider was live or a fixture, and it
 * doesn't score task success, so those tiers stay explicitly unestablished.
 */
export function evidenceTiers(o: OutcomeView): { tier: string; status: string }[] {
  const accepted = o.candidate?.decision === "adopted" && !!o.candidate.acceptance_ref;
  const rejected = o.candidate?.decision === "not_applicable" && !!o.candidate.acceptance_ref;
  return [
    {
      tier: "local acceptance",
      status: accepted ? "passed (recorded)" : rejected ? "failed (recorded)" : "none recorded",
    },
    { tier: "provider execution", status: "live vs fixture not recorded by the backend" },
    { tier: "task success", status: "not measured" },
  ];
}

/**
 * Loads one adoption's candidate decisions on demand. Errors are kept apart
 * from an empty list: an unreadable log never reads as "no candidates".
 */
export function useCandidateDecisions(
  api: Api,
  adoptionId: string | null,
  open: boolean,
): { items: CandidateDecision[] | null; error: boolean } {
  const [result, setResult] = useState<{ id: string; items: CandidateDecision[] | null; error: boolean } | null>(
    null,
  );
  const latest = useRef<string | null>(null);
  useEffect(() => {
    latest.current = adoptionId;
    if (!open || !adoptionId) return;
    api
      .get<{ adoption_id: string; items: CandidateDecision[] }>(
        `/capability-adoptions/${encodeURIComponent(adoptionId)}/candidates`,
      )
      .then((r) => {
        if (latest.current !== adoptionId || r.adoption_id !== adoptionId || !Array.isArray(r.items)) return;
        setResult({ id: adoptionId, items: r.items, error: false });
      })
      .catch(() => {
        if (latest.current === adoptionId) setResult({ id: adoptionId, items: null, error: true });
      });
  }, [api, adoptionId, open]);
  if (!result || result.id !== adoptionId) return { items: null, error: false };
  return { items: result.items, error: result.error };
}
