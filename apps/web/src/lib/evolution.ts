// How the organization changed shape over time. Folds the durable event log
// into "generations": one per revision in which the structure changed (roles
// created or retired, duties moved, capabilities attached, contractors
// engaged). Each generation names its cause from the change that committed it
// and the observations that triggered that change. Nothing is inferred
// beyond the recorded events and change records.

import type { DomainEvent, OrgChangeRecord, OrgState, Provider } from "./contracts";

export type DeltaKind = "role_added" | "role_retired" | "duty_moved" | "capability" | "contractor" | "reporting";

export interface Delta {
  kind: DeltaKind;
  text: string;
  /** Provider of an added role, when known. */
  provider?: Provider;
  /** The role was provisioned by a development stub. */
  devStub?: boolean;
}

export interface Generation {
  revision: number;
  /** Short label: "Founded", "Grew", "Reshaped", "Shrank", "Equipped", "Contracted", "Rewired". */
  label: string;
  /** Why the organization changed, from the change rationale or mission goal. */
  cause: string;
  /** Observation summaries that triggered the change, if any. */
  triggers: string[];
  changeId?: string;
  submittedBy?: string;
  /** Planner model that proposed the change, when a planner decision names it. */
  plannerModel?: string;
  deltas: Delta[];
  /** Active roles after this revision. */
  activeRoles: number;
}

const STRUCTURAL = new Set<DomainEvent["type"]>([
  "mission_created",
  "role_activated",
  "role_retired",
  "responsibility_transferred",
  "capability_attached",
  "contract_state_changed",
  "reporting_edge_changed",
]);

/** Ordered generations, oldest first. */
export function generations(events: DomainEvent[], changes: OrgChangeRecord[], state: OrgState | null): Generation[] {
  const sorted = [...events].sort((a, b) => a.seq - b.seq);
  const byRevision = new Map<number, DomainEvent[]>();
  for (const e of sorted) {
    const list = byRevision.get(e.revision);
    if (list) list.push(e);
    else byRevision.set(e.revision, [e]);
  }

  const names = new Map<string, string>();
  const providers = new Map<string, Provider>();
  const stubs = new Set<string>();
  for (const e of sorted) {
    if (e.type === "role_created") {
      names.set(e.payload.role_id, e.payload.name);
      providers.set(e.payload.role_id, e.payload.provider);
    }
    if (e.type === "role_provisioned" && e.payload.dev_stub) stubs.add(e.payload.role_id);
  }
  const name = (id: string | null | undefined) => (id ? (names.get(id) ?? state?.roles?.[id]?.name ?? id) : "—");
  const purposes = new Map<string, string>();
  for (const e of sorted) if (e.type === "responsibility_created") purposes.set(e.payload.responsibility_id, e.payload.purpose);
  const purpose = (id: string) => purposes.get(id) ?? state?.responsibilities?.[id]?.purpose ?? id;
  const contracts = new Map<string, string>();
  for (const e of sorted) if (e.type === "contract_requested") contracts.set(e.payload.contract_id, `${e.payload.scope} (${e.payload.specialist_address})`);
  const observations = new Map<string, string>();
  for (const e of sorted) if (e.type === "observation_recorded") observations.set(e.payload.observation_id, e.payload.summary);

  const committedAt = new Map<number, OrgChangeRecord>();
  for (const c of changes) if (c.status === "COMMITTED" && c.committed_revision != null) committedAt.set(c.committed_revision, c);

  const out: Generation[] = [];
  let active = 0;
  const revisions = [...byRevision.keys()].sort((a, b) => a - b);
  for (const rev of revisions) {
    const evs = byRevision.get(rev)!;
    const deltas: Delta[] = [];
    let mission: string | undefined;
    let added = 0;
    let retired = 0;
    for (const e of evs) {
      switch (e.type) {
        case "mission_created":
          mission = e.payload.goal;
          break;
        case "role_activated": {
          active += 1;
          added += 1;
          const id = e.payload.role_id;
          deltas.push({ kind: "role_added", text: name(id), provider: providers.get(id), devStub: stubs.has(id) });
          break;
        }
        case "role_retired":
          active -= 1;
          retired += 1;
          deltas.push({ kind: "role_retired", text: name(e.payload.role_id) });
          break;
        case "responsibility_transferred":
          deltas.push({
            kind: "duty_moved",
            text: `${purpose(e.payload.responsibility_id)}: ${name(e.payload.from)} → ${name(e.payload.to)}`,
          });
          break;
        case "capability_attached":
          deltas.push({
            kind: "capability",
            text: `${e.payload.capability.capability_id}@${e.payload.capability.version} → ${name(e.payload.role_id)}`,
          });
          break;
        case "contract_state_changed":
          if (e.payload.to === "ACCEPTED") deltas.push({ kind: "contractor", text: `Contractor engaged: ${contracts.get(e.payload.contract_id) ?? e.payload.contract_id}` });
          break;
        case "reporting_edge_changed":
          deltas.push({
            kind: "reporting",
            text: `${name(e.payload.role_id)} now reports to ${e.payload.reports_to ? name(e.payload.reports_to) : "no one"}`,
          });
          break;
        default:
          break;
      }
    }
    if (!evs.some((e) => STRUCTURAL.has(e.type)) || (deltas.length === 0 && !mission)) continue;

    const change = committedAt.get(rev);
    const decision = change
      ? sorted.find((e) => e.type === "planner_decision" && e.payload.change_id === change.change_id)
      : undefined;
    const label = mission
      ? "Founded"
      : added > 0 && retired > 0
        ? "Reshaped"
        : added > 0
          ? "Grew"
          : retired > 0
            ? "Shrank"
            : deltas.every((d) => d.kind === "capability")
              ? "Equipped"
              : deltas.every((d) => d.kind === "contractor")
                ? "Contracted"
                : "Rewired";
    out.push({
      revision: rev,
      label,
      cause: change?.proposal.rationale ?? mission ?? "",
      triggers: (change?.proposal.trigger_evidence_ids ?? []).map((id) => observations.get(id) ?? id),
      changeId: change?.change_id,
      submittedBy: change?.submitted_by,
      plannerModel: decision?.type === "planner_decision" ? decision.payload.model : undefined,
      deltas,
      activeRoles: active,
    });
  }
  return out;
}
