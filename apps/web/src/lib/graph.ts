// Pure helpers that turn organization state into a reporting tree.

import type {
  AgentBinding,
  ChangeStatus,
  OrgChangeRecord,
  OrgState,
  Provider,
  Responsibility,
  Role,
} from "./contracts";

/** A role that exists only inside an in-flight change. Never executable. */
export interface PendingRole {
  role_id: string;
  name: string;
  charter: string;
  provider: Provider;
  reports_to: string | null;
  change_id: string;
  change_status: ChangeStatus;
  binding: AgentBinding | null;
}

export type RoleNode =
  | { kind: "role"; id: string; role: Role; children: RoleNode[] }
  | { kind: "pending"; id: string; pending: PendingRole; children: RoleNode[] };

const IN_FLIGHT: ChangeStatus[] = ["VALIDATED", "PROVISIONING", "READY"];

export function pendingRoles(changes: OrgChangeRecord[], state: OrgState | null): PendingRole[] {
  const out: PendingRole[] = [];
  for (const c of changes) {
    if (!IN_FLIGHT.includes(c.status)) continue;
    for (const op of c.proposal.operations) {
      if (op.op !== "CreateRole") continue;
      const roleId = c.allocation.roles?.[op.key];
      if (!roleId || state?.roles?.[roleId]) continue;
      let reportsTo: string | null = null;
      if (op.reports_to?.kind === "existing") reportsTo = op.reports_to.role_id;
      if (op.reports_to?.kind === "new") reportsTo = c.allocation.roles?.[op.reports_to.key] ?? null;
      out.push({
        role_id: roleId,
        name: op.name,
        charter: op.charter,
        provider: op.provider,
        reports_to: reportsTo,
        change_id: c.change_id,
        change_status: c.status,
        binding: c.provisioned?.[op.key] ?? null,
      });
    }
  }
  return out;
}

export function buildTree(
  state: OrgState,
  pending: PendingRole[],
  opts: { showRetired: boolean },
): RoleNode[] {
  const roles = Object.values(state.roles ?? {}).filter(
    (r) => opts.showRetired || (r.state !== "RETIRED" && r.state !== "PROVISIONING_FAILED"),
  );
  const nodes = new Map<string, RoleNode>();
  for (const r of roles) nodes.set(r.role_id, { kind: "role", id: r.role_id, role: r, children: [] });
  for (const p of pending) nodes.set(p.role_id, { kind: "pending", id: p.role_id, pending: p, children: [] });

  const parentOf = (n: RoleNode): string | null | undefined =>
    n.kind === "role" ? n.role.reports_to : n.pending.reports_to;

  const roots: RoleNode[] = [];
  for (const n of nodes.values()) {
    const parent = parentOf(n);
    const p = parent ? nodes.get(parent) : undefined;
    if (p && p !== n) p.children.push(n);
    else roots.push(n);
  }
  const order = (n: RoleNode) =>
    n.kind === "role" ? n.role.created_in_revision * 10 + (n.role.state === "RETIRED" ? 5 : 0) : 1e12;
  const sortRec = (list: RoleNode[]) => {
    list.sort((a, b) => order(a) - order(b));
    list.forEach((c) => sortRec(c.children));
  };
  sortRec(roots);
  return roots;
}

export function ownedBy(state: OrgState, roleId: string): Responsibility[] {
  return Object.values(state.responsibilities ?? {}).filter(
    (r) => r.owner.kind === "role" && r.owner.id === roleId && r.status === "OPEN",
  );
}

export function executedBy(state: OrgState, roleId: string): Responsibility[] {
  return Object.values(state.responsibilities ?? {}).filter(
    (r) => r.executor?.kind === "role" && r.executor.id === roleId && r.status === "OPEN",
  );
}

/** For a dev-stub binding, which provider it stands in for (from `devstub-<provider>-...`). */
export function devStubTarget(binding: AgentBinding | null | undefined): string | null {
  if (!binding || binding.provider !== "dev_stub") return null;
  const m = binding.provider_agent_id.match(/^devstub-([a-z_]+)-/);
  return m?.[1] ?? "unknown";
}

export function roleName(state: OrgState | null, id: string | null | undefined): string {
  if (!id) return "—";
  return state?.roles?.[id]?.name ?? short(id);
}

export function short(id: string, n = 8): string {
  const i = id.indexOf("_");
  if (i < 0) return id.length > n + 4 ? `${id.slice(0, n + 4)}…` : id;
  return `${id.slice(0, i + 1)}${id.slice(i + 1, i + 1 + n)}`;
}
