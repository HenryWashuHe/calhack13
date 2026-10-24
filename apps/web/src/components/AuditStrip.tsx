import { useMemo, useState } from "react";
import type { Api } from "../lib/api";
import type { OrgState } from "../lib/contracts";
import { AuditCheck, auditSnapshot, auditSummary } from "../lib/audit";

type Sweep =
  | { kind: "idle" }
  | { kind: "running"; done: number; total: number }
  | { kind: "done"; results: { revision: number; failed: AuditCheck[] | null }[] };

/**
 * Re-checks the constitution rules against the snapshot on screen, and on
 * request against every stored revision. A second reader of the kernel's
 * committed snapshots, not the authority that enforces the rules.
 */
export function AuditStrip({
  api,
  state,
  current,
  onSelect,
}: {
  api: Api;
  state: OrgState;
  current: number;
  onSelect: (revision: number | null) => void;
}) {
  const checks = useMemo(() => auditSnapshot(state), [state]);
  const sum = auditSummary(checks);
  const [open, setOpen] = useState(false);
  const [sweep, setSweep] = useState<Sweep>({ kind: "idle" });

  async function auditAll() {
    const total = current + 1;
    const results: { revision: number; failed: AuditCheck[] | null }[] = [];
    setSweep({ kind: "running", done: 0, total });
    for (let r = 0; r <= current; r++) {
      try {
        const s = await api.get<OrgState>(`/organization/revisions/${r}`);
        results.push({ revision: r, failed: auditSnapshot(s).filter((c) => !c.holds) });
      } catch {
        results.push({ revision: r, failed: null });
      }
      setSweep({ kind: "running", done: r + 1, total });
    }
    setSweep({ kind: "done", results });
  }

  const sweepOk = sweep.kind === "done" && sweep.results.every((r) => r.failed !== null && r.failed.length === 0);

  return (
    <div className={`audit${sum.ok ? " ok" : " bad"}`}>
      <div className="audit-head">
        <button className="audit-toggle" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
          <span className="audit-mark" aria-hidden="true">
            {sum.ok ? "✓" : "✗"}
          </span>
          Constitution audit: {sum.held}/{sum.total} rules hold at r{state.revision}
        </button>
        {sweep.kind === "idle" && (
          <button className="btn" onClick={auditAll}>
            Audit all {current + 1} revisions
          </button>
        )}
        {sweep.kind === "running" && (
          <span className="muted tabular">
            auditing {sweep.done}/{sweep.total}…
          </span>
        )}
        {sweep.kind === "done" && (
          <span className={sweepOk ? "audit-ok" : "audit-bad"}>
            {sweepOk
              ? `every revision r0–r${sweep.results.length - 1} holds`
              : `${sweep.results.filter((r) => r.failed === null || r.failed.length > 0).length} revision(s) need attention`}
          </span>
        )}
      </div>
      {sweep.kind === "done" && (
        <div className="audit-cells" role="list" aria-label="Audit by revision">
          {sweep.results.map((r) => {
            const tone = r.failed === null ? "unknown" : r.failed.length === 0 ? "ok" : "bad";
            const title =
              r.failed === null
                ? `r${r.revision}: snapshot unavailable`
                : r.failed.length === 0
                  ? `r${r.revision}: all rules hold`
                  : `r${r.revision}: ${r.failed.map((c) => c.title).join(", ")}`;
            return (
              <button
                key={r.revision}
                role="listitem"
                className={`audit-cell ${tone}${r.revision === state.revision ? " here" : ""}`}
                title={title}
                aria-label={title}
                onClick={() => onSelect(r.revision === current ? null : r.revision)}
              >
                {r.revision}
              </button>
            );
          })}
        </div>
      )}
      {open && (
        <ul className="audit-list">
          {checks.map((c) => (
            <li key={c.id} className={c.holds ? "ok" : "bad"}>
              <span className="audit-mark" aria-hidden="true">
                {c.holds ? "✓" : "✗"}
              </span>
              <div>
                <div className="audit-title">{c.title}</div>
                <div className="muted">{c.rule}</div>
                {c.violations.map((v) => (
                  <div key={v} className="audit-violation">
                    {v}
                  </div>
                ))}
              </div>
            </li>
          ))}
          <li className="audit-note muted">
            Checked in the browser against the kernel's stored snapshot. The kernel enforces these rules at commit; this
            is an independent second reading.
          </li>
        </ul>
      )}
    </div>
  );
}
