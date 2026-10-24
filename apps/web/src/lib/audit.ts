// Independent audit of a stored organization snapshot. The kernel enforces
// these rules when it validates and commits a change (genesis-core engine
// post-checks, ownership transfer and retirement). This module re-checks them
// in the browser, against each committed snapshot the kernel serves, so an
// operator can see for any revision that self-modification left the
// organization within its constitution. It is a second reader, not the
// authority: a failure here means the snapshot and the rules disagree and
// should be investigated.

import type { OrgState, Role } from "./contracts";

export interface AuditCheck {
  id: string;
  title: string;
  /** The rule in one sentence. */
  rule: string;
  holds: boolean;
  /** Human-readable counterexamples, empty when the rule holds. */
  violations: string[];
}

const live = (r: Role) => r.state !== "RETIRED" && r.state !== "PROVISIONING_FAILED";
const ACTIVE_CONTRACT = new Set(["REQUESTED", "OFFERED", "ACCEPTED"]);

function check(id: string, title: string, rule: string, violations: string[]): AuditCheck {
  return { id, title, rule, holds: violations.length === 0, violations };
}

export function auditSnapshot(state: OrgState): AuditCheck[] {
  const roles = state.roles ?? {};
  const liveRoles = Object.values(roles).filter(live);
  const name = (id: string) => roles[id]?.name ?? id;
  const c = state.constitution;
  const open = Object.values(state.responsibilities ?? {}).filter((r) => r.status === "OPEN");
  const assignments = Object.values(state.assignments ?? {});
  const out: AuditCheck[] = [];

  // 1. Every open duty has exactly one live accountable owner.
  out.push(
    check(
      "owner",
      "Every open duty has a live owner",
      "An open responsibility is owned by the mission controller or by a role that is not retired.",
      open
        .filter((r) => r.owner.kind === "role" && !(roles[r.owner.id] && live(roles[r.owner.id]!)))
        .map((r) => `${r.purpose} is owned by ${r.owner.kind === "role" ? name(r.owner.id) : "?"}, which is not live`),
    ),
  );

  // 2. The owner holds the duty under the current epoch; stale holders are fenced.
  const fencing: string[] = [];
  for (const r of open) {
    const active = assignments.filter((a) => a.responsibility_id === r.responsibility_id && a.state === "ACTIVE");
    if (r.owner.kind !== "role") {
      if (active.length > 0) fencing.push(`${r.purpose} has an active assignment but no role owner`);
      continue;
    }
    if (active.length !== 1) {
      fencing.push(`${r.purpose} has ${active.length} active assignments, expected 1`);
      continue;
    }
    const a = active[0]!;
    if (a.role_id !== r.owner.id) fencing.push(`${r.purpose} is assigned to ${name(a.role_id)}, not its owner ${name(r.owner.id)}`);
    if (a.epoch !== r.epoch) fencing.push(`${r.purpose} assignment epoch ${a.epoch} ≠ duty epoch ${r.epoch}`);
  }
  for (const a of assignments) {
    if (a.state !== "ACTIVE") continue;
    const r = state.responsibilities?.[a.responsibility_id];
    if (!r || r.status !== "OPEN") fencing.push(`assignment ${a.assignment_id} is active on a closed duty`);
    const holder = roles[a.role_id];
    if (holder && !live(holder)) fencing.push(`${holder.name} is retired but still holds ${r?.purpose ?? a.responsibility_id}`);
  }
  out.push(
    check(
      "fencing",
      "One current holder per duty",
      "Each open role-owned duty has exactly one active assignment, held by its owner at the current epoch; nothing else is active.",
      fencing,
    ),
  );

  // 3. Requirement ownership follows its responsibility.
  out.push(
    check(
      "requirement",
      "Requirements follow their duty",
      "A requirement served by an open responsibility has the same accountable owner.",
      open
        .filter((r) => r.requirement_id && state.requirements?.[r.requirement_id])
        .filter((r) => JSON.stringify(state.requirements![r.requirement_id!]!.owner) !== JSON.stringify(r.owner))
        .map((r) => `${state.requirements![r.requirement_id!]!.summary} is owned differently from ${r.purpose}`),
    ),
  );

  // 4. Budget: live roles never hold more than the constitution allows.
  const allocated = liveRoles.reduce((s, r) => s + (r.authority.budget_cents ?? 0), 0);
  out.push(
    check(
      "budget",
      "Budget within the constitution",
      "The budget held by live roles is at most the constitution budget.",
      allocated > c.budget_cents ? [`roles hold ${allocated}¢, constitution allows ${c.budget_cents}¢`] : [],
    ),
  );

  // 5. Authority: no live role holds a scope the constitution does not grant.
  const allowed = new Set(c.scopes);
  out.push(
    check(
      "authority",
      "Authority within the constitution",
      "Every scope a live role holds is listed in the constitution.",
      liveRoles.flatMap((r) =>
        (r.authority.scopes ?? []).filter((s) => !allowed.has(s)).map((s) => `${r.name} holds ungranted scope ${s}`),
      ),
    ),
  );

  // 6. Size limits.
  const contracts = Object.values(state.contracts ?? {}).filter((k) => ACTIVE_CONTRACT.has(k.state)).length;
  out.push(
    check(
      "limits",
      "Size limits respected",
      "Live roles and open contracts stay within the constitution's limits.",
      [
        ...(liveRoles.length > c.max_active_roles ? [`${liveRoles.length} live roles > limit ${c.max_active_roles}`] : []),
        ...(contracts > c.max_active_contracts ? [`${contracts} open contracts > limit ${c.max_active_contracts}`] : []),
      ],
    ),
  );

  // 7. Reporting lines form a forest of live roles.
  const reporting: string[] = [];
  for (const r of liveRoles) {
    if (!r.reports_to) continue;
    const boss = roles[r.reports_to];
    if (!boss || !live(boss)) reporting.push(`${r.name} reports to ${boss?.name ?? r.reports_to}, which is not live`);
    const seen = new Set<string>([r.role_id]);
    let cur = boss;
    while (cur?.reports_to) {
      if (seen.has(cur.role_id)) {
        reporting.push(`${r.name} is in a reporting cycle`);
        break;
      }
      seen.add(cur.role_id);
      cur = roles[cur.reports_to];
    }
  }
  out.push(
    check("reporting", "Reporting lines are a tree", "Live roles report to live roles, with no cycles.", reporting),
  );

  // 8. Delegated executors exist and are live.
  out.push(
    check(
      "executor",
      "Executors are live",
      "An open duty delegated to a role is executed by a live role; one delegated to a contractor names a known contract.",
      open.flatMap((r) => {
        const e = r.executor;
        if (!e) return [];
        if (e.kind === "role") return roles[e.id] && live(roles[e.id]!) ? [] : [`${r.purpose} is executed by ${name(e.id)}, not live`];
        return state.contracts?.[e.id] ? [] : [`${r.purpose} names unknown contract ${e.id}`];
      }),
    ),
  );

  return out;
}

export function auditSummary(checks: AuditCheck[]): { held: number; total: number; ok: boolean } {
  const held = checks.filter((c) => c.holds).length;
  return { held, total: checks.length, ok: held === checks.length };
}
