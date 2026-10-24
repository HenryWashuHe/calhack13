// Why does this role exist? Reconstructs the evidence chain behind a role
// from persisted records only: the observations that triggered the change,
// the requirement it serves, the planner's decision, the change itself, how
// its agent was provisioned and proven ready, and what happened to it since.
// Every step cites the identifiers it came from; nothing is inferred beyond
// what the kernel recorded.

import type { DomainEvent, OrgChangeRecord, OrgOp, OrgState } from "./contracts";
import { devStubTarget } from "./graph";

export type StepKind =
  | "mission"
  | "observation"
  | "requirement"
  | "planner"
  | "change"
  | "provisioning"
  | "readiness"
  | "commit"
  | "ownership"
  | "capability"
  | "retirement";

export interface ProvenanceStep {
  kind: StepKind;
  title: string;
  detail: string;
  refs: string[];
  /** Development stand-in, never live provider evidence. */
  devStub?: boolean;
  tone?: "ok" | "warn" | "bad";
}

export interface RoleProvenance {
  roleId: string;
  name: string;
  /** How the role came to exist. */
  origin: "mission" | "change" | "unknown";
  changeId?: string;
  /** The causal chain up to the role's creation, oldest first. */
  cause: ProvenanceStep[];
  /** What happened to the role afterwards, oldest first. */
  since: ProvenanceStep[];
}

/** The committed change whose allocation created `roleId`, with the role's key. */
export function creatingChange(
  roleId: string,
  changes: OrgChangeRecord[],
): { change: OrgChangeRecord; key: string } | null {
  for (const c of changes) {
    if (c.status !== "COMMITTED") continue;
    for (const [key, id] of Object.entries(c.allocation?.roles ?? {})) {
      if (id === roleId) return { change: c, key };
    }
  }
  return null;
}

/** Requirements a change assigned to the new role `key` (via from_requirement targets). */
function requirementsFor(ops: OrgOp[], key: string): string[] {
  const out: string[] = [];
  for (const op of ops) {
    if (op.op !== "AssignResponsibility") continue;
    if (op.owner.kind !== "new" || op.owner.key !== key) continue;
    if (op.target.kind === "from_requirement") out.push(op.target.requirement_id);
  }
  return out;
}

/** Existing responsibilities a change hands to the new role `key` (retirement handoffs, reassignments). */
function inheritedBy(ops: OrgOp[], key: string): { responsibilityId: string; from: string }[] {
  const out: { responsibilityId: string; from: string }[] = [];
  for (const op of ops) {
    if (op.op === "RetireRole") {
      for (const h of op.handoffs ?? []) {
        if (h.to.kind === "new" && h.to.key === key) out.push({ responsibilityId: h.responsibility_id, from: op.role_id });
      }
    } else if (op.op === "ReassignResponsibility" && op.to.kind === "new" && op.to.key === key) {
      out.push({ responsibilityId: op.responsibility_id, from: op.from });
    }
  }
  return out;
}

export function roleProvenance(
  roleId: string,
  state: OrgState | null,
  changes: OrgChangeRecord[],
  events: DomainEvent[],
): RoleProvenance {
  const role = state?.roles?.[roleId];
  const name = role?.name ?? roleId;
  const cause: ProvenanceStep[] = [];
  const created = creatingChange(roleId, changes);

  if (created) {
    const { change: c, key } = created;
    const p = c.proposal;
    for (const obsId of p.trigger_evidence_ids ?? []) {
      const o = state?.observations?.[obsId];
      cause.push({
        kind: "observation",
        title: o ? `Observed (${o.source})` : "Observation",
        detail: o ? o.summary : "observation not in the current snapshot",
        refs: [obsId, ...(o?.evidence_ref ? [o.evidence_ref] : [])],
      });
    }
    for (const reqId of requirementsFor(p.operations, key)) {
      const r = state?.requirements?.[reqId];
      cause.push({
        kind: "requirement",
        title: "Unmet requirement",
        detail: r ? `${r.summary} — done when: ${r.acceptance}` : "requirement not in the current snapshot",
        refs: [reqId],
      });
    }
    for (const h of inheritedBy(p.operations, key)) {
      const r = state?.responsibilities?.[h.responsibilityId];
      cause.push({
        kind: "ownership",
        title: `Inherits a duty from ${state?.roles?.[h.from]?.name ?? h.from}`,
        detail: r ? r.purpose : "responsibility not in the current snapshot",
        refs: [h.responsibilityId],
      });
    }
    const decision = events.find((e) => e.type === "planner_decision" && e.payload.change_id === c.change_id);
    if (decision && decision.type === "planner_decision") {
      const d = decision.payload;
      cause.push({
        kind: "planner",
        title: `Planner proposed it (${d.model})`,
        detail: `${d.summary} — ${d.rationale}`,
        refs: [`attempt ${d.attempt}`, `${d.latency_ms} ms`],
      });
    }
    const createOp = p.operations.find(
      (op): op is Extract<OrgOp, { op: "CreateRole" }> => op.op === "CreateRole" && op.key === key,
    );
    cause.push({
      kind: "change",
      title: `Organization change by ${c.submitted_by}`,
      detail:
        p.rationale +
        (createOp?.justification ? ` (separate role justified by ${createOp.justification.replace(/_/g, " ")})` : ""),
      refs: [c.change_id, `expected r${p.expected_org_revision}`, ...p.operations.map((o) => o.op)],
    });
    const binding = c.provisioned?.[key];
    const op = c.operations?.[key];
    if (binding) {
      const stubFor = devStubTarget(binding);
      const stub = binding.provider === "dev_stub" || stubFor !== null;
      const attached = op?.native_status === "operator_attached";
      cause.push({
        kind: "provisioning",
        title: stub
          ? `Provisioned by a development stub${stubFor && stubFor !== "unknown" ? ` standing in for ${stubFor}` : ""}`
          : attached
            ? `Agent ID attached by an operator (${binding.provider})`
            : `Agent created on ${binding.provider}`,
        detail: op?.run_id ? `provider run ${op.run_id}` : "",
        refs: [binding.provider_agent_id, binding.config_digest].filter(Boolean),
        devStub: stub,
        tone: stub || attached ? "warn" : undefined,
      });
    }
    if (op?.readiness_evidence) {
      cause.push({
        kind: "readiness",
        title: "Readiness proven",
        detail: "backend-observed evidence",
        refs: [op.readiness_evidence],
        tone: "ok",
      });
    }
    cause.push({
      kind: "commit",
      title: `Committed in r${c.committed_revision ?? role?.created_in_revision ?? "?"}`,
      detail: "The role became active only when the whole change committed.",
      refs: [],
      tone: "ok",
    });
  } else if (role) {
    // Created with a mission bootstrap: the mission created in the same
    // revision as the role. Without events, fall back to the mission of the
    // oldest-epoch duty it still owns (a bootstrap duty is never transferred
    // to its first owner).
    const createdAt = events.find((e) => e.type === "role_created" && e.payload.role_id === roleId)?.revision;
    const boot =
      createdAt !== undefined
        ? events.find((e) => e.type === "mission_created" && e.revision === createdAt)
        : undefined;
    const owned = Object.values(state?.responsibilities ?? {})
      .filter((r) => r.owner.kind === "role" && r.owner.id === roleId)
      .sort((a, b) => a.epoch - b.epoch);
    const missionId =
      boot?.type === "mission_created" ? boot.payload.mission_id : owned.length ? owned[0]!.mission_id : undefined;
    const mission = missionId ? state?.missions?.[missionId] : undefined;
    cause.push({
      kind: "mission",
      title: mission ? "Created with the mission" : "Created at bootstrap",
      detail: mission ? mission.goal : `present since r${role.created_in_revision}`,
      refs: mission ? [mission.mission_id] : [],
    });
  }

  // What happened since: ownership moves, capabilities, retirement.
  const since: ProvenanceStep[] = [];
  const purpose = (rid: string) => state?.responsibilities?.[rid]?.purpose ?? rid;
  const myGaps = new Map<string, string>();
  const createdIn = created?.change.committed_revision ?? undefined;
  for (const e of events) {
    switch (e.type) {
      case "capability_gap_opened":
        if (e.payload.role_id === roleId) {
          myGaps.set(e.payload.gap_id, e.payload.capability_id);
          since.push({
            kind: "capability",
            title: "Asked for a capability",
            detail: `${e.payload.capability_id} for ${purpose(e.payload.responsibility_id)}`,
            refs: [e.payload.gap_id, e.payload.requirement_id],
          });
        }
        break;
      case "capability_gap_status_changed": {
        const cap = myGaps.get(e.payload.gap_id);
        const to = e.payload.to;
        if (cap === undefined) break;
        if (to.state === "installed") {
          since.push({
            kind: "capability",
            title: "Requested capability installed",
            detail: `${cap} as ${to.capability.capability_id}@${to.capability.version}`,
            refs: [e.payload.gap_id, to.installation_id],
            tone: "ok",
          });
        } else if (to.state === "refused") {
          since.push({
            kind: "capability",
            title: "Requested capability refused",
            detail: `${cap}: ${to.reason}`,
            refs: [e.payload.gap_id],
            tone: "bad",
          });
        }
        break;
      }
      case "responsibility_transferred":
        // A handoff in the creating commit is already the "Inherits a duty" cause.
        if (e.payload.to === roleId && createdIn !== undefined && e.revision === createdIn) break;
        if (e.payload.to === roleId) {
          since.push({
            kind: "ownership",
            title: "Took over a responsibility",
            detail: `${purpose(e.payload.responsibility_id)} (from ${state?.roles?.[e.payload.from]?.name ?? e.payload.from}, epoch ${e.payload.epoch})`,
            refs: [e.payload.responsibility_id],
          });
        } else if (e.payload.from === roleId) {
          since.push({
            kind: "ownership",
            title: "Handed off a responsibility",
            detail: `${purpose(e.payload.responsibility_id)} (to ${state?.roles?.[e.payload.to]?.name ?? e.payload.to}, epoch ${e.payload.epoch})`,
            refs: [e.payload.responsibility_id],
          });
        }
        break;
      case "capability_attached":
        if (e.payload.role_id === roleId) {
          since.push({
            kind: "capability",
            title: "Capability attached",
            detail: `${e.payload.capability.capability_id}@${e.payload.capability.version}`,
            refs: [],
          });
        }
        break;
      case "role_retired":
        if (e.payload.role_id === roleId) {
          const retiring = changes.find(
            (c) =>
              c.status === "COMMITTED" &&
              c.proposal.operations.some((op) => op.op === "RetireRole" && op.role_id === roleId),
          );
          since.push({
            kind: "retirement",
            title: `Retired in r${e.revision}`,
            detail: retiring ? retiring.proposal.rationale : "",
            refs: retiring ? [retiring.change_id] : [],
          });
        }
        break;
      default:
        break;
    }
  }

  return {
    roleId,
    name,
    origin: created ? "change" : role ? "mission" : "unknown",
    changeId: created?.change.change_id,
    cause,
    since,
  };
}
