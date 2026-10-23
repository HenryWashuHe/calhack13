// Requested capabilities (capability gaps) for the console. Pure helpers.
import type { CapabilityGap, CapabilityGapStatus, OrgState } from "./contracts";
import { intakeDetail, type GapIntakeStatus } from "./gapIntake";
type Tone = "ok" | "warn" | "bad";

export interface GapView {
  gapId: string;
  capabilityId: string;
  purpose: string;
  label: string;
  tone: Tone | "neutral";
  /** Exact version once verified, e.g. `listing-reader@1`. */
  version: string | null;
  detail: string | null;
  sources: string[];
}

/** One label per lifecycle state. Verified or published is not installed. */
export function gapLabel(s: CapabilityGapStatus): { label: string; tone: GapView["tone"]; detail: string | null; version: string | null } {
  switch (s.state) {
    case "open":
      return { label: "REQUESTED", tone: "neutral", detail: "waiting for an operator-approved acquisition policy", version: null };
    case "acquiring":
      return { label: "ACQUIRING", tone: "warn", detail: `acquisition ${s.acquisition_id}`, version: null };
    case "acquired":
      return {
        label: "VERIFIED",
        tone: "warn",
        detail: s.publication_ref ? `published ${s.publication_ref}; not installed here yet` : "verified; not installed here yet",
        version: `${s.capability.capability_id}@${s.capability.version}`,
      };
    case "installed":
      return {
        label: "INSTALLED",
        tone: "ok",
        detail: `installation ${s.installation_id}`,
        version: `${s.capability.capability_id}@${s.capability.version}`,
      };
    case "refused":
      return { label: "REFUSED", tone: "bad", detail: s.reason, version: null };
  }
}

/**
 * Gaps requested for one role, oldest first. With the server's intake status,
 * open and acquiring gaps say what automatic acquisition is doing.
 */
export function gapsForRole(
  state: OrgState | null,
  roleId: string,
  intake: Record<string, GapIntakeStatus> = {},
): GapView[] {
  return Object.values(state?.capability_gaps ?? {})
    .filter((g: CapabilityGap) => g.role_id === roleId)
    .sort((a, b) => a.opened_in_revision - b.opened_in_revision || a.gap_id.localeCompare(b.gap_id))
    .map((g) => {
      const l = gapLabel(g.status);
      return {
        gapId: g.gap_id,
        capabilityId: g.capability_id,
        purpose: g.purpose,
        label: l.label,
        tone: l.tone,
        version: l.version,
        detail: intakeDetail(g, intake[g.gap_id]) ?? l.detail,
        sources: g.observed_sources,
      };
    });
}
