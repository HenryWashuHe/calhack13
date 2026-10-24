// The kernel's audit of an organization's stored history, as served by
// `GET /api/v1/organization/audit` (genesis_server::audit::audit_history).
// The console only displays it; the rules live in genesis_core::audit.

export interface RevisionViolation {
  revision: number;
  invariant: string;
  detail: string;
}

export interface HistoryAudit {
  org_id: string;
  snapshots_checked: number;
  first_revision: number | null;
  last_revision: number | null;
  events_checked: number;
  /** Every invariant applied: core state, core transition and storage rules. */
  invariants: string[];
  violation_count: number;
  /** The first violations, up to the server's reporting cap. */
  violations: RevisionViolation[];
}

export type RevisionVerdict = "ok" | "bad" | "unknown";

/**
 * Verdict per stored revision. When the server truncated its violation list,
 * an unreported violation can sit at any revision (snapshot violations are
 * listed first, then event-log ones), so no unflagged revision is called clean.
 */
export function verdicts(a: HistoryAudit): { revision: number; verdict: RevisionVerdict; violations: RevisionViolation[] }[] {
  if (a.first_revision === null || a.last_revision === null) return [];
  const truncated = a.violation_count > a.violations.length;
  const out = [];
  for (let r = a.first_revision; r <= a.last_revision; r++) {
    const vs = a.violations.filter((v) => v.revision === r);
    const verdict: RevisionVerdict = vs.length > 0 ? "bad" : truncated ? "unknown" : "ok";
    out.push({ revision: r, verdict, violations: vs });
  }
  return out;
}

/** "budget_limit" -> "budget limit". */
export const ruleName = (invariant: string) => invariant.replace(/_/g, " ");
