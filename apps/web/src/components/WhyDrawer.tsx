import { useMemo } from "react";
import type { DomainEvent, OrgChangeRecord, OrgState } from "../lib/contracts";
import { ProvenanceStep, roleProvenance } from "../lib/provenance";

const LABEL: Record<ProvenanceStep["kind"], string> = {
  mission: "Mission",
  observation: "Evidence",
  requirement: "Need",
  planner: "Planner",
  change: "Change",
  provisioning: "Provider",
  readiness: "Ready",
  commit: "Commit",
  ownership: "Ownership",
  capability: "Capability",
  retirement: "Retired",
};

function Step({ step }: { step: ProvenanceStep }) {
  return (
    <li className={`why-step why-${step.kind}${step.tone ? ` ${step.tone}` : ""}`}>
      <span className="why-kind">{LABEL[step.kind]}</span>
      <div className="why-body">
        <div className="why-title">
          {step.title}
          {step.devStub && <span className="chip warn">dev stub, not live</span>}
        </div>
        {step.detail && <div className="why-detail">{step.detail}</div>}
        {step.refs.length > 0 && (
          <div className="why-refs">
            {step.refs.map((r) => (
              <span key={r} className="mono">
                {r}
              </span>
            ))}
          </div>
        )}
      </div>
    </li>
  );
}

/**
 * Answers "why does this role exist?" from persisted records: the evidence,
 * the unmet requirement, the planner's decision, the change, provisioning,
 * readiness and commit — then what happened to the role since.
 */
export function WhyDrawer({
  roleId,
  state,
  changes,
  events,
}: {
  roleId: string;
  state: OrgState;
  changes: OrgChangeRecord[];
  events: DomainEvent[];
}) {
  const p = useMemo(() => roleProvenance(roleId, state, changes, events), [roleId, state, changes, events]);
  return (
    <div className="why" aria-label={`Why ${p.name} exists`}>
      {p.cause.length === 0 ? (
        <p className="muted">No recorded cause for this role in the current snapshot.</p>
      ) : (
        <ol className="why-chain">
          {p.cause.map((s, i) => (
            <Step key={`c${i}`} step={s} />
          ))}
        </ol>
      )}
      {p.since.length > 0 && (
        <>
          <div className="why-sep">Since then</div>
          <ol className="why-chain">
            {p.since.map((s, i) => (
              <Step key={`s${i}`} step={s} />
            ))}
          </ol>
        </>
      )}
    </div>
  );
}
