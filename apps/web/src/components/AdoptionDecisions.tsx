// Henry's Claude: one gap's adoption decision trail, under its row in the
// organization tree. Shows the server's conclusion first, then (on demand)
// every candidate the registry offered with this organization's decision and
// reason, the local acceptance reference, and which evidence tiers this view
// does and doesn't establish. Score is shown as ordering only.

import { useState } from "react";
import type { Api } from "../lib/api";
import {
  DECISION,
  evidenceTiers,
  OUTCOME,
  useCandidateDecisions,
  type OutcomeView,
} from "../lib/adoptionDecisions";

export function AdoptionDecisions({
  api,
  adoptionId,
  outcome,
}: {
  api: Api | null;
  adoptionId: string | null;
  outcome: OutcomeView;
}) {
  const [open, setOpen] = useState(false);
  const { items, error } = useCandidateDecisions(api ?? NO_API, adoptionId, open && !!api);
  const o = OUTCOME[outcome.outcome];
  const c = outcome.candidate;
  return (
    <div className="adoption-decisions" style={{ width: "100%" }}>
      <span className={`chip ${o.tone}`} title={o.meaning}>
        adoption: {o.label}
      </span>
      {c && (
        <span className="mono" title={`decision: ${DECISION[c.decision].label}`}>
          {c.capability_id}@{c.version} from {c.publisher}
        </span>
      )}
      <span className="muted">{outcome.reason}</span>
      {api && adoptionId && (
        <button className="btn" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
          {open ? "Hide decisions" : `Decisions (${outcome.considered})`}
        </button>
      )}
      {open && (
        <div className="decision-trail">
          <ul className="evidence-tiers" aria-label="Evidence tiers">
            {evidenceTiers(outcome).map((t) => (
              <li key={t.tier}>
                <span className="chip neutral">{t.tier}</span> <span className="muted">{t.status}</span>
              </li>
            ))}
            <li>
              <span className="chip neutral">time and cost</span> <span className="muted">not measured</span>
            </li>
          </ul>
          {error && <p className="muted">The decision log couldn't be read. This is not the same as no candidates.</p>}
          {!error && items === null && <p className="muted">Loading decisions…</p>}
          {!error && items !== null && items.length === 0 && (
            <p className="muted">No candidates were recorded for this adoption.</p>
          )}
          {!error && items !== null && items.length > 0 && (
            <table className="decisions">
              <thead>
                <tr>
                  <th title="Registry order by similarity; ordering only, never applicability">rank</th>
                  <th>candidate</th>
                  <th>publisher</th>
                  <th>decision</th>
                  <th>why</th>
                  <th>local acceptance</th>
                </tr>
              </thead>
              <tbody>
                {items.map((d) => (
                  <tr key={d.rank}>
                    <td title={d.score === null ? "score not finite" : `score ${d.score} (ordering only)`}>
                      {d.rank + 1}
                    </td>
                    <td className="mono">
                      {d.capability_id}@{d.version}
                    </td>
                    <td className="mono">{d.publisher}</td>
                    <td>
                      <span className={`chip ${DECISION[d.decision].tone}`}>{DECISION[d.decision].label}</span>
                    </td>
                    <td>
                      {d.reason}
                      {d.missing_scopes.length > 0 && ` (${d.missing_scopes.join(", ")})`}
                    </td>
                    <td className="mono">{d.acceptance_ref ?? "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}
    </div>
  );
}

const NO_API = { get: () => Promise.reject(new Error("no api")) } as unknown as Api;
