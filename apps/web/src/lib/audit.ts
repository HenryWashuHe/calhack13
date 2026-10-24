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
 * Verdict per revision in the audited range. Unflagged revisions are called
 * clean only when the report proves it covered them: when the server truncated
 * its violation list (snapshot violations are listed first, then event-log
 * ones) an unreported violation can sit at any revision, and when fewer
 * snapshots were checked than the range spans, some revision in it was never
 * stored or checked (the report doesn't say which), so neither case is "ok".
 */
export function verdicts(
  a: HistoryAudit,
): { revision: number; verdict: RevisionVerdict; violations: RevisionViolation[]; note?: string }[] {
  if (a.first_revision === null || a.last_revision === null) return [];
  const truncated = a.violation_count > a.violations.length;
  const complete = a.snapshots_checked === a.last_revision - a.first_revision + 1;
  const note = !complete
    ? "not verified (some revisions in this range were never stored or checked)"
    : "not reported (violation list truncated)";
  const out = [];
  for (let r = a.first_revision; r <= a.last_revision; r++) {
    const vs = a.violations.filter((v) => v.revision === r);
    const verdict: RevisionVerdict = vs.length > 0 ? "bad" : truncated || !complete ? "unknown" : "ok";
    out.push(verdict === "unknown" ? { revision: r, verdict, violations: vs, note } : { revision: r, verdict, violations: vs });
  }
  return out;
}

/** True when the report covers every revision in its range with no violation. */
export const allPass = (a: HistoryAudit) =>
  a.violation_count === 0 &&
  a.first_revision !== null &&
  a.last_revision !== null &&
  a.snapshots_checked === a.last_revision - a.first_revision + 1;

/** "budget_limit" -> "budget limit". */
export const ruleName = (invariant: string) => invariant.replace(/_/g, " ");
