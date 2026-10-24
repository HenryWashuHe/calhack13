import { useMemo, useState } from "react";
import type { DomainEvent, OrgState } from "../lib/contracts";
import { Category, describe } from "../lib/describe";

const FILTERS: { key: Category | "all"; label: string }[] = [
  { key: "all", label: "All" },
  { key: "organization", label: "Organization" },
  { key: "changes", label: "Changes" },
  { key: "jobs", label: "Jobs" },
  { key: "capabilities", label: "Capabilities" },
  { key: "evidence", label: "Evidence" },
];

export function EvidencePanel({ events, state }: { events: DomainEvent[]; state: OrgState | null }) {
  const [filter, setFilter] = useState<Category | "all">("all");
  const rows = useMemo(
    () =>
      events
        .map((e) => ({ e, d: describe(e, state) }))
        .filter(({ d }) => filter === "all" || d.category === filter)
        .reverse(),
    [events, state, filter],
  );
  return (
    <section className="panel evidence" aria-labelledby="ev-h">
      <header>
        <h2 id="ev-h">Evidence log</h2>
        <span className="muted tabular">{events.length} events</span>
      </header>
      <div className="panel-body">
        <div className="filters" role="tablist" aria-label="Filter events">
          {FILTERS.map((f) => (
            <button
              key={f.key}
              className="deck-tab"
              role="tab"
              aria-selected={filter === f.key}
              onClick={() => setFilter(f.key)}
            >
              {f.label}
            </button>
          ))}
        </div>
        {rows.length === 0 && <p className="empty">No events.</p>}
        <ol className="events">
          {rows.map(({ e, d }) => (
            <li key={e.seq}>
              <span className="seq" title={new Date(e.timestamp).toLocaleString()}>
                {e.seq}
              </span>
              <div>
                <div className={`title ${d.tone ?? ""}`}>
                  {d.title}
                  <span className="mono muted">r{e.revision}</span>
                  {d.devStub && <span className="chip warn">DEV STUB</span>}
                </div>
                {d.detail && <div className="detail">{d.detail}</div>}
                {d.refs.length > 0 && (
                  <div className="refs">
                    {d.refs.map((r) => (
                      <code key={r}>{r}</code>
                    ))}
                  </div>
                )}
              </div>
            </li>
          ))}
        </ol>
      </div>
    </section>
  );
}
