// Adopt-before-acquire (`open` → `installed` by adoption) for the console:
// role-row detail and Attention items from the server's operator-only
// `GET /capability-gaps/{id}/adoption` (#82). The kernel's gap lifecycle stays
// the label of record; nothing here infers installation. Servers without the
// route answer 404, which is ignored.

import { createContext, useEffect, useMemo, useRef, useState } from "react";
import type { Api } from "./api";
import type { AttentionItem, AttentionKind } from "./attention";
import type { CapabilityGap, OrgState } from "./contracts";
import { roleName } from "./graph";

export type AdoptionState = "active" | "released" | "installed" | "attention" | "refused";
export type AdoptionCode =
  | "configuration"
  | "authority"
  | "registry"
  | "candidate"
  | "checks"
  | "rejected"
  | "no_candidate"
  | "conflict"
  | "invalid_record";

/** Mirrors the server's `GapAdoptionStatus`. */
export interface GapAdoptionStatus {
  gap_id: string;
  enabled: boolean;
  /** `null` until the coordinator freezes a record for this gap. */
  state: AdoptionState | null;
  code: AdoptionCode | null;
  adoption_id: string | null;
  synthesis_fallback: boolean | null;
  selected: { capability_id: string; version: string } | null;
  installation_id: string | null;
  updated_at: string | null;
}

export const ADOPTION_POLL_MS = 3000;

export const GapAdoptionContext = createContext<Record<string, GapAdoptionStatus>>({});

/** Open gaps: the only ones adopt-first can still be working on. */
export function openGaps(state: OrgState | null): CapabilityGap[] {
  return Object.values(state?.capability_gaps ?? {})
    .filter((g) => g.status.state === "open")
    .sort((a, b) => a.opened_in_revision - b.opened_in_revision || a.gap_id.localeCompare(b.gap_id));
}

const CODE: Record<AdoptionCode, { kind: AttentionKind; text: string }> = {
  configuration: { kind: "action", text: "adoption suite, registry or executor configuration changed" },
  authority: { kind: "action", text: "the requirement or its assignment changed" },
  registry: {
    kind: "unknown",
    text: "the registry search failed or was ambiguous; nothing falls back to synthesis",
  },
  candidate: { kind: "action", text: "the registry offered an invalid or unusable capability" },
  checks: {
    kind: "unknown",
    text: "an acceptance check failed or its provider outcome is unknown; nothing is resubmitted",
  },
  rejected: { kind: "action", text: "this organization's suite rejected the adopted capability" },
  no_candidate: { kind: "action", text: "no compatible published capability was found" },
  conflict: { kind: "action", text: "the adopted version conflicts with an existing installation" },
  invalid_record: { kind: "action", text: "stored adoption records disagree with the frozen intent" },
};

/**
 * One line for an open gap's row, or null to fall through to the next source
 * (gap intake, then the default). Takes precedence over intake while adoption
 * owns the gap, since the gap driver is deferred then.
 */
export function adoptionDetail(gap: CapabilityGap, s: GapAdoptionStatus | undefined): string | null {
  if (!s || gap.status.state !== "open" || s.state === null) return null;
  switch (s.state) {
    case "active":
      if (!s.adoption_id)
        return s.code === "conflict"
          ? "adopt first · waiting for another adoption on this requirement to finish"
          : "adopt first · preparing one adoption before any synthesis";
      return `adopt first · adoption ${s.adoption_id}: searching published capabilities, then this organization's own checks`;
    case "attention":
      return s.code ? `adopt first · ${CODE[s.code].text}${s.adoption_id ? ` (adoption ${s.adoption_id})` : ""}` : null;
    case "released":
      // A definitive empty search handed the gap to synthesis; intake says the rest.
      return null;
    default:
      return null;
  }
}

/** Open Attention items for gaps whose adoption stopped. */
export function adoptionAttention(
  state: OrgState | null,
  statuses: Record<string, GapAdoptionStatus>,
): AttentionItem[] {
  const out: AttentionItem[] = [];
  for (const g of openGaps(state)) {
    const s = statuses[g.gap_id];
    if (!s || s.state !== "attention" || !s.code) continue;
    const c = CODE[s.code];
    out.push({
      key: `gap-adoption:${g.gap_id}:${s.code}`,
      kind: c.kind,
      title: `Adopting ${g.capability_id} stopped: operator needed`,
      detail: `${c.text} · requested by ${roleName(state, g.role_id)}`,
      refs: [g.gap_id, ...(s.adoption_id ? [s.adoption_id] : [])],
    });
  }
  return out;
}

/**
 * Polls adoption status for every open gap while any exist. Responses from a
 * previous organization, for another gap, or older than the newest applied
 * one are dropped.
 */
export function useGapAdoption(api: Api, state: OrgState | null): Record<string, GapAdoptionStatus> {
  const [statuses, setStatuses] = useState<Record<string, GapAdoptionStatus>>({});
  const ids = useMemo(() => openGaps(state).map((g) => g.gap_id), [state]);
  const key = ids.join(",");
  const generation = useRef(0);
  const sequence = useRef(0);
  const applied = useRef(new Map<string, number>());
  useEffect(() => {
    generation.current += 1;
    applied.current = new Map();
    setStatuses({});
  }, [api]);
  useEffect(() => {
    if (ids.length === 0) return;
    const mine = generation.current;
    let stopped = false;
    const poll = () => {
      for (const id of ids) {
        const seq = ++sequence.current;
        api
          .get<GapAdoptionStatus>(`/capability-gaps/${encodeURIComponent(id)}/adoption`)
          .then((s) => {
            if (stopped || generation.current !== mine || s.gap_id !== id) return;
            if ((applied.current.get(id) ?? 0) > seq) return;
            applied.current.set(id, seq);
            setStatuses((p) => ({ ...p, [id]: s }));
          })
          .catch(() => undefined);
      }
    };
    poll();
    const t = setInterval(poll, ADOPTION_POLL_MS);
    return () => {
      stopped = true;
      clearInterval(t);
    };
    // `key` stands for `ids`.
  }, [api, key]);
  return statuses;
}
