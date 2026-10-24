// Gap finalization (`acquired` → `installed`) for the console: role-row detail
// and Attention items from the server's operator-only
// `GET /capability-gaps/{id}/finalization`. The kernel's gap lifecycle stays
// the label of record; nothing here infers installation.

import { createContext, useEffect, useMemo, useRef, useState } from "react";
import type { Api } from "./api";
import type { AttentionItem, AttentionKind } from "./attention";
import type { CapabilityGap, OrgState } from "./contracts";
import { roleName } from "./graph";

export type FinalizerState = "active" | "attention" | "installed" | "refused";
export type FinalizerCode =
  | "configuration"
  | "acquisition"
  | "authority"
  | "publication"
  | "installation"
  | "checks"
  | "invalid_record";

/** Mirrors the server's `FinalizationStatus`. */
export interface GapFinalizationStatus {
  gap_id: string;
  enabled: boolean;
  /** `null` until the finalizer freezes a record for this gap. */
  state: FinalizerState | null;
  code: FinalizerCode | null;
  capability: { capability_id: string; version: string } | null;
  publication_ref: string | null;
  installation_id: string | null;
  job_ids: string[];
  updated_at: string | null;
}

export const FINALIZATION_POLL_MS = 3000;

export const GapFinalizationContext = createContext<Record<string, GapFinalizationStatus>>({});

/** Verified gaps the finalizer may still act on. */
export function verifiedGaps(state: OrgState | null): CapabilityGap[] {
  return Object.values(state?.capability_gaps ?? {})
    .filter((g) => g.status.state === "acquired")
    .sort((a, b) => a.opened_in_revision - b.opened_in_revision || a.gap_id.localeCompare(b.gap_id));
}

const CODE: Record<FinalizerCode, { kind: AttentionKind; text: string }> = {
  configuration: {
    kind: "action",
    text: "finalization rule, manifest or acceptance suite changed since it was frozen; fix the server configuration",
  },
  acquisition: { kind: "action", text: "the acquisition is no longer verified for this request" },
  authority: { kind: "action", text: "the requesting role's assignment or permissions changed" },
  publication: {
    kind: "action",
    text: "publication failed or the registry does not hold exactly the verified version",
  },
  installation: { kind: "action", text: "the local installation could not be created or does not match" },
  checks: {
    kind: "unknown",
    text: "a local check run failed or its provider outcome is unknown; reconcile it, nothing is resubmitted",
  },
  invalid_record: { kind: "action", text: "stored records disagree with the frozen finalization" },
};

/** One line for an acquired gap's row, or null to keep the default. */
export function finalizationDetail(gap: CapabilityGap, s: GapFinalizationStatus | undefined): string | null {
  if (!s || gap.status.state !== "acquired" || !s.enabled) return null;
  // No record: the server has no rule for this capability (or hasn't frozen
  // one yet). Enablement alone says nothing about this gap, so claim nothing.
  if (s.state === null) return null;
  if (s.state === "attention") return s.code ? `verified · ${CODE[s.code].text}` : null;
  if (s.state !== "active") return null;
  if (!s.publication_ref) return "verified; publishing this exact version";
  if (!s.installation_id) return `published ${s.publication_ref}; installing here`;
  if (s.job_ids.length === 0) return `installation ${s.installation_id}; queuing local acceptance checks`;
  return `installation ${s.installation_id}; ${s.job_ids.length} local acceptance check${s.job_ids.length === 1 ? "" : "s"} running`;
}

/** Open Attention items for verified gaps whose finalization stopped. */
export function finalizationAttention(
  state: OrgState | null,
  statuses: Record<string, GapFinalizationStatus>,
): AttentionItem[] {
  const out: AttentionItem[] = [];
  for (const g of verifiedGaps(state)) {
    const s = statuses[g.gap_id];
    if (!s || s.state !== "attention" || !s.code) continue;
    const c = CODE[s.code];
    out.push({
      key: `gap-finalization:${g.gap_id}:${s.code}`,
      kind: c.kind,
      title: `Verified capability ${g.capability_id} is not installed: operator needed`,
      detail: `${c.text} · requested by ${roleName(state, g.role_id)}`,
      refs: [g.gap_id, ...(s.installation_id ? [s.installation_id] : []), ...s.job_ids],
    });
  }
  return out;
}

/**
 * Polls finalization status for every verified gap while any exist. Same
 * rules as the intake hook: responses from a previous organization, for
 * another gap, or older than the newest applied one are dropped.
 */
export function useGapFinalization(api: Api, state: OrgState | null): Record<string, GapFinalizationStatus> {
  const [statuses, setStatuses] = useState<Record<string, GapFinalizationStatus>>({});
  const ids = useMemo(() => verifiedGaps(state).map((g) => g.gap_id), [state]);
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
          .get<GapFinalizationStatus>(`/capability-gaps/${encodeURIComponent(id)}/finalization`)
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
    const t = setInterval(poll, FINALIZATION_POLL_MS);
    return () => {
      stopped = true;
      clearInterval(t);
    };
    // `key` stands for `ids`.
  }, [api, key]);
  return statuses;
}
