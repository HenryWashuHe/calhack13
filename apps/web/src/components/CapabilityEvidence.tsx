import type { Installation, Job, OrgState } from "../lib/contracts";
import { capabilityKey, holders, partitionJobs, summarizeRun, summarizeVerification } from "../lib/capabilities";
import { NOT_MANAGED, type CapabilityEvidence as Evidence } from "../lib/useCapabilityEvidence";
import { DEV_ASSERTION } from "../lib/capabilities";
import { short } from "../lib/graph";
import { StateChip } from "./Chips";

function Manifest({ inst, evidence }: { inst: Installation; evidence: Evidence }) {
  const loaded = evidence.manifests[capabilityKey(inst.capability)];
  if (!loaded) return <p className="empty">Loading the canonical manifest…</p>;
  if ("error" in loaded) {
    return <div className="notice bad">Canonical manifest unavailable: {loaded.error}</div>;
  }
  const m = loaded.value;
  const v = summarizeVerification(m);
  return (
    <div className="ev-grid">
      <div>
        <h4>Pinned build</h4>
        <div className="mono">
          Actor {m.artifact.actor_id} · build {m.artifact.build_number}
        </div>
        <div className="mono muted">build id {m.artifact.build_id}</div>
        <div className="mono muted" title={m.source_digest}>
          source {short(m.source_digest, 18)} · publisher {m.publisher}
        </div>
      </div>
      <div>
        <h4>Publisher verification</h4>
        <div>
          <span className={`chip ${v.allPassed ? "ok" : "bad"}`}>
            {v.passed}/{v.total} cases passed
          </span>{" "}
          {v.devAssertion && (
            <span className="chip warn" title="Published from a development assertion, not backend-computed evidence">
              DEVELOPMENT ASSERTION
            </span>
          )}
        </div>
        <div className="muted">
          {v.environment}
          {v.oracle && <span className="mono"> · oracle {v.oracle}</span>}
        </div>
        {v.limitations.length > 0 && (
          <ul className="ev-limits">
            {v.limitations.map((l, i) => (
              <li key={i}>{l}</li>
            ))}
          </ul>
        )}
      </div>
      <div>
        <h4>Local acceptance</h4>
        <div style={{ display: "flex", gap: 6, flexWrap: "wrap", alignItems: "center" }}>
          <StateChip state={inst.status} />
          {inst.acceptance_report_ref?.startsWith(DEV_ASSERTION) && <span className="chip warn">DEVELOPMENT ASSERTION</span>}
        </div>
        <span className="mono muted">{inst.acceptance_report_ref ?? "no accepted report yet"}</span>
      </div>
    </div>
  );
}

function Runs({ jobs, evidence, empty = "No pinned runs yet." }: { jobs: Job[]; evidence: Evidence; empty?: string }) {
  if (jobs.length === 0) return <p className="empty">{empty}</p>;
  return (
    <div className="table-wrap">
      <table className="ev-runs">
        <thead>
          <tr>
            <th>Job</th>
            <th>Purpose</th>
            <th>Case</th>
            <th>Job status</th>
            <th>Run phase</th>
            <th>Provider run</th>
            <th>Output</th>
          </tr>
        </thead>
        <tbody>
          {jobs.map((j) => {
            const loaded = evidence.runs[j.job_id];
            const s = loaded && "value" in loaded ? summarizeRun(loaded.value) : null;
            return (
              <tr key={j.job_id}>
                <td className="mono">{short(j.job_id, 10)}</td>
                <td>{s?.purpose ?? "—"}</td>
                <td className="mono">{s?.caseId ?? "—"}</td>
                <td>
                  <StateChip state={j.status} />
                </td>
                <td>
                  {loaded && "error" in loaded ? (
                    <span className="muted">
                      {loaded.error === NOT_MANAGED ? "no backend run record (development job)" : "record unavailable"}
                    </span>
                  ) : s ? (
                    <>
                      <span className={`chip ${s.phaseTone ?? "neutral"}`}>{s.phase.replace(/_/g, " ")}</span>
                      {s.failure && <span className="sub mono">{s.failure}</span>}
                      {s.attention && <span className="sub">{s.attention}</span>}
                    </>
                  ) : (
                    <span className="muted">loading…</span>
                  )}
                </td>
                <td className="mono">
                  {s?.runId ? <span className="sub">run {s.runId}</span> : "—"}
                  {s?.datasetId && <span className="sub">dataset {s.datasetId}</span>}
                  {s?.providerStatus && <span className="sub">{s.providerStatus}</span>}
                </td>
                <td>{s?.output ?? "—"}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

export function CapabilityEvidence({
  state,
  jobs,
  evidence,
}: {
  state: OrgState | null;
  jobs: Job[];
  evidence: Evidence;
}) {
  const installs = Object.values(state?.installations ?? {});
  if (installs.length === 0) return null;
  const records = Object.fromEntries(
    Object.entries(evidence.runs).map(([id, l]) => [id, "value" in l ? l.value : undefined]),
  );
  return (
    <div style={{ display: "grid", gap: 12 }}>
      <p className="muted" style={{ margin: 0, fontSize: 12 }}>
        Collected runs stay in VERIFYING: provider completion is evidence, not task success. Acceptance is computed by
        this organization's backend from its own runs.
      </p>
      {installs.map((i) => {
        const owned = holders(state, i);
        const split = partitionJobs(jobs, i, records);
        return (
          <article key={i.installation_id} className="change ev-card">
            <div className="change-head">
              <span className="mono">{capabilityKey(i.capability)}</span>
              <span className="muted" style={{ fontSize: 12 }}>
                {owned.length > 0 ? `attached to ${owned.join(", ")}` : "not attached to a role"}
              </span>
            </div>
            <Manifest inst={i} evidence={evidence} />
            <h4 className="ev-sub">Acceptance runs for this installation</h4>
            <Runs jobs={split.bound} evidence={evidence} empty="No acceptance runs bound to this installation yet." />
            {split.unbound.length > 0 && (
              <>
                <h4 className="ev-sub">Other runs of this capability version</h4>
                <p className="muted" style={{ margin: 0, fontSize: 12 }}>
                  Not bound to a specific installation (invocations, or records still loading). Not acceptance evidence
                  for this installation.
                </p>
                <Runs jobs={split.unbound} evidence={evidence} />
              </>
            )}
          </article>
        );
      })}
    </div>
  );
}
