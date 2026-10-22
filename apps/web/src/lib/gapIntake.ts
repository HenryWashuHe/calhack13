// Automatic gap intake (the server's gap-to-acquisition driver) for the
// console. Pure helpers plus a polling hook. The status is operational
// metadata from `GET /capability-gaps/{id}/acquisition`; the kernel's gap
// lifecycle stays authoritative, and nothing here infers success.

import { createContext, useEffect, useMemo, useRef, useState } from "react";
import type { Api } from "./api";
import type { AttentionItem, AttentionKind } from "./attention";
import type { CapabilityGap, OrgState } from "./contracts";
import { roleName } from "./graph";

export type IntakeState =
  | "disabled"
  | "ready"
  | "waiting"
  | "attention"
  | "acquiring"
  | "acquired"
  | "installed"
  | "refused";

export type IntakeCode =
  | "configuration"
  | "policy"
  | "provisioning"
  | "authority"
  | "quota"
  | "conflict"
  | "invalid_record"
  | "unknown_outcome"
  | "retry_read"
  | "verification_rejected"
  | "acquisition_failed";

/** Mirrors the server's `GapDriverStatus`. */
export interface GapIntakeStatus {
  gap_id: string;
  enabled: boolean;
  state: IntakeState;
  code: IntakeCode | null;
  acquisition_id: string | null;
  updated_at: string | null;
}

export const INTAKE_POLL_MS = 3000;

/** Latest intake status per gap ID for the selected organization. */
export const GapIntakeContext = createContext<Record<string, GapIntakeStatus>>({});

/** Gaps the driver may still act on: open or acquiring. */
export function liveGaps(state: OrgState | null): CapabilityGap[] {
  return Object.values(state?.capability_gaps ?? {})
    .filter((g) => g.status.state === "open" || g.status.state === "acquiring")
    .sort((a, b) => a.opened_in_revision - b.opened_in_revision || a.gap_id.localeCompare(b.gap_id));
}

/** What an operator must know or do for each attention code. */
const CODE: Record<IntakeCode, { kind: AttentionKind; text: string }> = {
  configuration: {
    kind: "action",
    text: "acquisition policy, model settings or suite are missing or changed; fix the server configuration",
  },
  policy: { kind: "action", text: "capability or sources are outside the configured acquisition policy" },
  provisioning: { kind: "waiting", text: "waiting for the requesting role to become active" },
  authority: { kind: "action", text: "the requirement or acquisition authority is no longer valid" },
  quota: { kind: "action", text: "acquisition quota exhausted; nothing more is spent until it is raised" },
  conflict: { kind: "action", text: "another acquisition is active for this requirement" },
  invalid_record: { kind: "action", text: "a stored acquisition record failed validation; inspect before continuing" },
  unknown_outcome: {
    kind: "unknown",
    text: "a provider submission's outcome is unknown; reconcile the acquisition, nothing is replayed",
  },
  retry_read: {
    kind: "action",
    text: "a provider read failed; retry it explicitly with retry_read (never automatic)",
  },
  verification_rejected: { kind: "action", text: "independent verification rejected the candidate" },
  acquisition_failed: { kind: "action", text: "the acquisition ended with a definite failure" },
};

/** One line for the role's requested-capability row, or null to keep the default. */
export function intakeDetail(gap: CapabilityGap, s: GapIntakeStatus | undefined): string | null {
  if (!s) return null;
  if (gap.status.state === "open") {
    if (!s.enabled) return "automatic acquisition is off on this server; an operator must start it";
    if (s.state === "waiting" || s.state === "attention") return s.code ? CODE[s.code].text : null;
    return "queued for automatic acquisition";
  }
  if (gap.status.state === "acquiring") {
    const where = `acquisition ${gap.status.acquisition_id}`;
    if (s.state === "attention" && s.code) return `${where} · ${CODE[s.code].text}`;
    return `${where} · generating, building and independently verifying`;
  }
  return null;
}

/** Open attention items for gaps the driver cannot advance on its own. */
export function intakeAttention(state: OrgState | null, statuses: Record<string, GapIntakeStatus>): AttentionItem[] {
  const out: AttentionItem[] = [];
  for (const g of liveGaps(state)) {
    const s = statuses[g.gap_id];
    if (!s || (s.state !== "attention" && s.state !== "waiting") || !s.code) continue;
    const c = CODE[s.code];
    const acq = g.status.state === "acquiring" ? g.status.acquisition_id : s.acquisition_id;
    out.push({
      key: `gap-intake:${g.gap_id}:${s.code}`,
      kind: c.kind,
      title:
        c.kind === "waiting"
          ? `Capability request ${g.capability_id} is waiting`
          : `Capability request ${g.capability_id} needs an operator`,
      detail: `${c.text} · requested by ${roleName(state, g.role_id)}`,
      refs: [g.gap_id, ...(acq ? [acq] : [])],
    });
  }
  return out;
}

/**
 * Polls the intake status of every live gap while any exist. Responses from a
 * previous organization or an older poll are dropped; a failed request keeps
 * the last known status.
 */
export function useGapIntake(api: Api, state: OrgState | null): Record<string, GapIntakeStatus> {
  const [statuses, setStatuses] = useState<Record<string, GapIntakeStatus>>({});
  const ids = useMemo(() => liveGaps(state).map((g) => g.gap_id), [state]);
  const key = ids.join(",");
  const generation = useRef(0);
  useEffect(() => {
    generation.current += 1;
    setStatuses({});
  }, [api]);
  useEffect(() => {
    if (ids.length === 0) return;
    const mine = generation.current;
    let stopped = false;
    const poll = () => {
      for (const id of ids) {
        api
          .get<GapIntakeStatus>(`/capability-gaps/${encodeURIComponent(id)}/acquisition`)
          .then((s) => {
            if (!stopped && generation.current === mine && s.gap_id === id) {
              setStatuses((p) => ({ ...p, [id]: s }));
            }
          })
          .catch(() => undefined);
      }
    };
    poll();
    const t = setInterval(poll, INTAKE_POLL_MS);
    return () => {
      stopped = true;
      clearInterval(t);
    };
    // `key` stands for `ids`.
  }, [api, key]);
  return statuses;
}
