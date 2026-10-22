import { useState } from "react";
import type { Api } from "../lib/api";
import { useCapabilityEvidence } from "../lib/useCapabilityEvidence";
import { CapabilityEvidence } from "./CapabilityEvidence";
import type { ChangeStatus, DomainEvent, Job, OrgChangeRecord, OrgState } from "../lib/contracts";
import type { Commitment } from "../lib/useOrg";
import { roleName, short } from "../lib/graph";
import { ProviderBadge, StateChip } from "./Chips";

const STEPS: ChangeStatus[] = ["PROPOSED", "VALIDATED", "PROVISIONING", "READY", "COMMITTED"];

function Pipeline({ status }: { status: ChangeStatus }) {
  const failed = status === "FAILED" || status === "REJECTED";
  // Failed changes stop after the furthest step they could have reached.
  const idx = failed ? -1 : STEPS.indexOf(status);
  return (
    <div className="pipeline" aria-label={`Status ${status}`}>
      {STEPS.map((s, i) => (
        <span key={s} className={i < idx ? "done" : i === idx ? "current" : ""}>
          {s}
        </span>
      ))}
      {failed && <span className="failed">{status}</span>}
    </div>
  );
}

function Changes({ changes, state }: { changes: OrgChangeRecord[]; state: OrgState | null }) {
  if (changes.length === 0) return <p className="empty">No organization changes yet.</p>;
  return (
    <div style={{ display: "grid", gap: 10 }}>
      {changes.map((c) => (
        <article key={c.change_id} className="change">
          <div className="change-head">
            <span className="mono">{short(c.change_id, 10)}</span>
            <Pipeline status={c.status} />
            <span className="muted" style={{ fontSize: 12 }}>
              by {c.submitted_by} · expects r{c.proposal.expected_org_revision}
              {c.committed_revision ? ` · committed r${c.committed_revision}` : ""}
            </span>
          </div>
          <div style={{ color: "var(--ink-2)" }}>{c.proposal.rationale}</div>
          <div className="ops">
            {c.proposal.operations.map((op, i) => (
              <span key={i} className="chip neutral" title={JSON.stringify(op, null, 2)}>
                {op.op}
                {"name" in op ? `: ${op.name}` : ""}
                {op.op === "RetireRole" ? `: ${roleName(state, op.role_id)}` : ""}
              </span>
            ))}
          </div>
          {Object.entries(c.provisioned ?? {}).length > 0 && (
            <div className="ops">
              {Object.entries(c.provisioned ?? {}).map(([key, b]) => (
                <span key={key} style={{ display: "inline-flex", gap: 6, alignItems: "center", flexWrap: "wrap", minWidth: 0 }}>
                  <span className="mono">{key}</span>
                  <ProviderBadge binding={b} />
                  <span className="mono muted">{b.provider_agent_id}</span>
                </span>
              ))}
            </div>
          )}
          {(c.errors ?? []).length > 0 && (
            <ul className="errors">
              {(c.errors ?? []).map((e, i) => (
                <li key={i}>
                  <span className="mono">{e.code}</span> {e.message}
                </li>
              ))}
            </ul>
          )}
        </article>
      ))}
    </div>
  );
}

function Jobs({ jobs, state }: { jobs: Job[]; state: OrgState | null }) {
  if (jobs.length === 0) return <p className="empty">No jobs dispatched.</p>;
  return (
    <div className="table-wrap">
      <table>
        <thead>
          <tr>
            <th>Job</th>
            <th>Role</th>
            <th>Task</th>
            <th>Status</th>
            <th>Provider status</th>
            <th>Epoch</th>
            <th>Provider refs</th>
          </tr>
        </thead>
        <tbody>
          {jobs.map((j) => (
            <tr key={j.job_id}>
              <td className="mono">{short(j.job_id, 10)}</td>
              <td>{roleName(state, j.role_id)}</td>
              <td>
                {j.task}
                {j.capability && (
                  <span className="sub mono">
                    {j.capability.capability_id}@{j.capability.version}
                  </span>
                )}
              </td>
              <td>
                <StateChip state={j.status} />
                {j.rejection_reason && <span className="sub">{j.rejection_reason}</span>}
              </td>
              <td className="mono">{j.raw_status ?? "—"}</td>
              <td className="mono tabular">{j.epoch}</td>
              <td className="mono">
                {Object.entries(j.provider_refs ?? {}).map(([k, v]) => (
                  <span key={k} className="sub">
                    {k}: {v}
                  </span>
                ))}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Capabilities({
  api,
  state,
  jobs,
  events,
  ledger,
}: {
  api: Api;
  state: OrgState | null;
  jobs: Job[];
  events: DomainEvent[];
  ledger: Commitment[];
}) {
  const evidence = useCapabilityEvidence(api, state, jobs);
  const installs = Object.values(state?.installations ?? {});
  const synthesis = new Map<string, number>();
  for (const e of events) {
    if (e.type === "capability_installed") synthesis.set(e.payload.installation_id, e.payload.synthesis_calls);
  }
  return (
    <div style={{ display: "grid", gap: 16 }}>
      {installs.length === 0 ? (
        <p className="empty">No capabilities installed.</p>
      ) : (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Capability</th>
                <th>Status</th>
                <th>Source digest</th>
                <th>Synthesis calls</th>
                <th>Local acceptance</th>
                <th>Attached to</th>
              </tr>
            </thead>
            <tbody>
              {installs.map((i) => {
                const holders = Object.values(state?.roles ?? {}).filter((r) =>
                  (r.capabilities ?? []).some(
                    (c) => c.capability_id === i.capability.capability_id && c.version === i.capability.version,
                  ),
                );
                return (
                  <tr key={i.installation_id}>
                    <td className="mono">
                      {i.capability.capability_id}@{i.capability.version}
                    </td>
                    <td>
                      <StateChip state={i.status} />
                    </td>
                    <td className="mono">{i.source_digest.slice(0, 16)}…</td>
                    <td className="mono tabular">{synthesis.get(i.installation_id) ?? "—"}</td>
                    <td className="mono">{i.acceptance_report_ref ?? "pending"}</td>
                    <td>{holders.map((h) => h.name).join(", ") || "—"}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      <CapabilityEvidence state={state} jobs={jobs} evidence={evidence} />
      <div>
        <h3 className="muted" style={{ fontSize: 11, letterSpacing: "0.08em", textTransform: "uppercase", margin: "0 0 6px" }}>
          Simulated commitment ledger
        </h3>
        {ledger.length === 0 ? (
          <p className="empty">No commitments recorded.</p>
        ) : (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Operation</th>
                  <th>Job</th>
                  <th>Recorded</th>
                </tr>
              </thead>
              <tbody>
                {ledger.map((c) => (
                  <tr key={c.operation_id}>
                    <td className="mono">{c.operation_id}</td>
                    <td className="mono">{short(c.job_id, 10)}</td>
                    <td className="tabular">{new Date(c.created_at).toLocaleTimeString()}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}

type Tab = "changes" | "jobs" | "capabilities";

export function Deck(props: {
  api: Api;
  changes: OrgChangeRecord[];
  jobs: Job[];
  state: OrgState | null;
  events: DomainEvent[];
  ledger: Commitment[];
}) {
  const [tab, setTab] = useState<Tab>("changes");
  const counts: Record<Tab, number> = {
    changes: props.changes.length,
    jobs: props.jobs.length,
    capabilities: Object.keys(props.state?.installations ?? {}).length,
  };
  return (
    <section className="panel deck" aria-label="Changes, jobs and capabilities">
      <header>
        <div className="deck-tabs" role="tablist">
          {(["changes", "jobs", "capabilities"] as Tab[]).map((t) => (
            <button key={t} className="deck-tab" role="tab" aria-selected={tab === t} onClick={() => setTab(t)}>
              {t[0]!.toUpperCase() + t.slice(1)} <span className="muted tabular">{counts[t]}</span>
            </button>
          ))}
        </div>
      </header>
      <div className="panel-body">
        {tab === "changes" && <Changes changes={props.changes} state={props.state} />}
        {tab === "jobs" && <Jobs jobs={props.jobs} state={props.state} />}
        {tab === "capabilities" && <Capabilities
            api={props.api}
            state={props.state}
            jobs={props.jobs}
            events={props.events}
            ledger={props.ledger}
          />}
      </div>
    </section>
  );
}
