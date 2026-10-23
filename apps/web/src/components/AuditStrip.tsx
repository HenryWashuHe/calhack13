import { useEffect, useMemo, useState } from "react";
import type { Api } from "../lib/api";
import { HistoryAudit, ruleName, verdicts } from "../lib/audit";

type Load = { kind: "loading" } | { kind: "ready"; audit: HistoryAudit } | { kind: "error"; message: string };

/**
 * The kernel's own audit of every stored revision, transition and event
 * (`GET /organization/audit`). Re-fetched when the organization advances.
 */
export function AuditStrip({
  api,
  shown,
  current,
  onSelect,
}: {
  api: Api;
  /** Revision on screen (live or historical). */
  shown: number;
  /** Latest committed revision; a change re-runs the audit. */
  current: number;
  onSelect: (revision: number | null) => void;
}) {
  const [load, setLoad] = useState<Load>({ kind: "loading" });
  const [open, setOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;
    api
      .get<HistoryAudit>("/organization/audit")
      .then((audit) => {
        if (!cancelled) setLoad({ kind: "ready", audit });
      })
      .catch((e: unknown) => {
        if (!cancelled) setLoad({ kind: "error", message: e instanceof Error ? e.message : String(e) });
      });
    return () => {
      cancelled = true;
    };
  }, [api, current]);

  if (load.kind === "loading") return <div className="audit">Running the kernel audit…</div>;
  if (load.kind === "error")
    return (
      <div className="audit bad" role="alert">
        Kernel audit unavailable: {load.message}
      </div>
    );
  return (
    <AuditView
      audit={load.audit}
      shown={shown}
      current={current}
      onSelect={onSelect}
      open={open}
      onToggle={() => setOpen((o) => !o)}
    />
  );
}

export function AuditView({
  audit: a,
  shown,
  current,
  onSelect,
  open,
  onToggle,
}: {
  audit: HistoryAudit;
  shown: number;
  current: number;
  onSelect: (revision: number | null) => void;
  open: boolean;
  onToggle: () => void;
}) {
  const rows = useMemo(() => verdicts(a), [a]);
  const ok = a.violation_count === 0;
  const here = rows.find((r) => r.revision === shown);
  return (
    <div className={`audit${ok ? " ok" : " bad"}`}>
      <div className="audit-head">
        <button className="audit-toggle" aria-expanded={open} onClick={onToggle}>
          <span className="audit-mark" aria-hidden="true">
            {ok ? "✓" : "✗"}
          </span>
          {ok
            ? `Kernel audit: every revision r${a.first_revision ?? 0}–r${a.last_revision ?? 0} passes ${a.invariants.length} invariants`
            : `Kernel audit: ${a.violation_count} violation${a.violation_count === 1 ? "" : "s"} in stored history`}
        </button>
        <span className="muted tabular">
          {a.snapshots_checked} snapshots · {a.events_checked} events
        </span>
      </div>
      <div className="audit-cells" role="list" aria-label="Audit by revision">
        {rows.map((r) => {
          const title =
            r.verdict === "ok"
              ? `r${r.revision}: passes`
              : r.verdict === "bad"
                ? `r${r.revision}: ${r.violations.map((v) => ruleName(v.invariant)).join(", ")}`
                : `r${r.revision}: not reported (violation list truncated)`;
          return (
            <button
              key={r.revision}
              role="listitem"
              className={`audit-cell ${r.verdict}${r.revision === shown ? " here" : ""}`}
              title={title}
              aria-label={title}
              onClick={() => onSelect(r.revision === current ? null : r.revision)}
            >
              {r.revision}
            </button>
          );
        })}
      </div>
      {open && (
        <div className="audit-list">
          {here && here.violations.length > 0 && (
            <ul>
              {here.violations.map((v, i) => (
                <li key={i} className="audit-violation">
                  r{v.revision} {ruleName(v.invariant)}: {v.detail}
                </li>
              ))}
            </ul>
          )}
          <div className="audit-rules">
            {a.invariants.map((n) => (
              <span key={n} className="chip neutral">
                {ruleName(n)}
              </span>
            ))}
          </div>
          <p className="audit-note muted">
            Computed by the kernel (genesis_core::audit) over the stored snapshots and event log. A clean audit means
            every state is well formed and every step was a legal move; it does not show that providers acted or that
            evidence is true.
          </p>
        </div>
      )}
    </div>
  );
}
