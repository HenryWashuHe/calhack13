import type { OrgState } from "../lib/contracts";
import { roleName } from "../lib/graph";
import { StateChip } from "./Chips";

function time(ts: string) {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

export function MissionPanel({ state }: { state: OrgState | null }) {
  const missions = Object.values(state?.missions ?? {});
  const observations = Object.values(state?.observations ?? {}).sort((a, b) =>
    a.observed_at.localeCompare(b.observed_at),
  );
  const requirements = Object.values(state?.requirements ?? {});

  return (
    <section className="panel mission" aria-labelledby="mission-h">
      <header>
        <h2 id="mission-h">Mission</h2>
      </header>
      <div className="panel-body">
        {missions.length === 0 && (
          <p className="empty">No mission yet. Create one with POST /api/v1/missions.</p>
        )}
        {missions.map((m) => (
          <div key={m.mission_id} className="kv" style={{ gap: 10 }}>
            <div style={{ display: "flex", gap: 8, alignItems: "flex-start" }}>
              <p className="goal">{m.goal}</p>
              <StateChip state={m.status} />
            </div>
            {m.constraints && m.constraints.length > 0 && (
              <div className="kv">
                <h3>Constraints</h3>
                <ul>
                  {m.constraints.map((c) => (
                    <li key={c}>{c}</li>
                  ))}
                </ul>
              </div>
            )}
            {m.success_checks && m.success_checks.length > 0 && (
              <div className="kv">
                <h3>Success checks</h3>
                <ul>
                  {m.success_checks.map((c) => (
                    <li key={c}>{c}</li>
                  ))}
                </ul>
              </div>
            )}
          </div>
        ))}

        <div className="kv">
          <h3>Evidence and requirements</h3>
          {observations.length === 0 && requirements.length === 0 && (
            <p className="empty">Nothing observed yet.</p>
          )}
          <ul className="timeline">
            {observations.map((o) => (
              <li key={o.observation_id}>
                <div className="row">
                  <span className="mono muted">{time(o.observed_at)}</span>
                  <span className="chip neutral">{o.source}</span>
                </div>
                <span>{o.summary}</span>
                {requirements
                  .filter((r) => r.triggering_evidence.includes(o.observation_id))
                  .map((r) => (
                    <div key={r.requirement_id} className="timeline" style={{ marginTop: 6 }}>
                      <div className="req" style={{ paddingLeft: 10, borderLeft: "2px solid var(--accent)" }}>
                        <div className="row">
                          <strong style={{ fontSize: 13 }}>Requirement</strong>
                          <StateChip state={r.status} />
                        </div>
                        <div>{r.summary}</div>
                        <div className="muted" style={{ fontSize: 12 }}>
                          Owner:{" "}
                          {r.owner.kind === "mission_controller" ? "mission controller" : roleName(state, r.owner.id)} ·
                          Done when: {r.acceptance}
                        </div>
                      </div>
                    </div>
                  ))}
              </li>
            ))}
          </ul>
        </div>
      </div>
    </section>
  );
}
