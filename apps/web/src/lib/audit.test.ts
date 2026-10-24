import { describe, expect, it } from "vitest";
import type { OrgState } from "./contracts";
import { auditSnapshot, auditSummary } from "./audit";

function base(): OrgState {
  return {
    org_id: "o",
    name: "O",
    revision: 4,
    constitution: { scopes: ["plan", "ledger"], budget_cents: 1000, max_active_roles: 3, max_active_contracts: 1 },
    requirements: {
      q: {
        requirement_id: "q",
        mission_id: "m",
        summary: "Find a venue",
        triggering_evidence: ["obs"],
        acceptance: "reserved",
        status: "ASSIGNED",
        owner: { kind: "role", id: "b" },
      },
    },
    responsibilities: {
      d1: { responsibility_id: "d1", mission_id: "m", purpose: "Coordinate", owner: { kind: "role", id: "a" }, trigger: "", termination: "", status: "OPEN", epoch: 1 },
      d2: {
        responsibility_id: "d2",
        mission_id: "m",
        purpose: "Secure a venue",
        requirement_id: "q",
        owner: { kind: "role", id: "b" },
        executor: { kind: "contractor", id: "k" },
        trigger: "",
        termination: "",
        status: "OPEN",
        epoch: 3,
      },
      d3: { responsibility_id: "d3", mission_id: "m", purpose: "Old work", owner: { kind: "role", id: "x" }, trigger: "", termination: "", status: "COMPLETED", epoch: 1 },
    },
    roles: {
      a: { role_id: "a", name: "Coordinator", charter: "", authority: { scopes: ["plan"], budget_cents: 500 }, state: "ACTIVE", reports_to: null, created_in_revision: 1 },
      b: { role_id: "b", name: "Scout", charter: "", authority: { scopes: ["ledger"], budget_cents: 400 }, state: "ACTIVE", reports_to: "a", created_in_revision: 4 },
      x: { role_id: "x", name: "Retired lead", charter: "", authority: { scopes: ["ghost"], budget_cents: 9000 }, state: "RETIRED", reports_to: "a", created_in_revision: 2, retired_in_revision: 3 },
    },
    assignments: {
      s1: { assignment_id: "s1", role_id: "a", responsibility_id: "d1", epoch: 1, state: "ACTIVE" },
      s2: { assignment_id: "s2", role_id: "b", responsibility_id: "d2", epoch: 3, state: "ACTIVE" },
      s0: { assignment_id: "s0", role_id: "x", responsibility_id: "d2", epoch: 2, state: "FENCED" },
      s3: { assignment_id: "s3", role_id: "x", responsibility_id: "d3", epoch: 1, state: "RELEASED" },
    },
    contracts: {
      k: { contract_id: "k", specialist_address: "agent1q", protocol: "p", scope: "shuttles", state: "ACCEPTED", supervising_role: "b" },
    },
  };
}

const failing = (s: OrgState) =>
  Object.fromEntries(auditSnapshot(s).filter((c) => !c.holds).map((c) => [c.id, c.violations]));

describe("auditSnapshot", () => {
  it("passes a consistent snapshot; retired roles' budgets and scopes do not count", () => {
    expect(failing(base())).toEqual({});
    expect(auditSummary(auditSnapshot(base()))).toEqual({ held: 8, total: 8, ok: true });
  });

  it("catches an orphaned duty", () => {
    const s = base();
    s.responsibilities!.d1!.owner = { kind: "role", id: "x" };
    s.assignments!.s1!.role_id = "x";
    const f = failing(s);
    expect(f.owner).toEqual(["Coordinate is owned by Retired lead, which is not live"]);
    expect(f.fencing).toContain("Retired lead is retired but still holds Coordinate");
  });

  it("catches a stale epoch, a second holder and a holder other than the owner", () => {
    const s = base();
    s.assignments!.s2!.epoch = 2;
    expect(failing(s).fencing).toEqual(["Secure a venue assignment epoch 2 ≠ duty epoch 3"]);
    s.assignments!.s0!.state = "ACTIVE";
    expect(failing(s).fencing?.[0]).toBe("Secure a venue has 2 active assignments, expected 1");
    const t = base();
    t.assignments!.s2!.role_id = "a";
    expect(failing(t).fencing).toEqual(["Secure a venue is assigned to Coordinator, not its owner Scout"]);
  });

  it("catches active assignments on closed duties", () => {
    const s = base();
    s.assignments!.s3!.state = "ACTIVE";
    expect(failing(s).fencing).toContain("assignment s3 is active on a closed duty");
  });

  it("catches a requirement whose owner drifted from its duty", () => {
    const s = base();
    s.requirements!.q!.owner = { kind: "mission_controller" };
    expect(failing(s).requirement).toEqual(["Find a venue is owned differently from Secure a venue"]);
  });

  it("catches budget, authority and size-limit breaches", () => {
    const s = base();
    s.roles!.b!.authority.budget_cents = 600;
    s.roles!.b!.authority.scopes = ["ledger", "payroll"];
    s.constitution.max_active_roles = 1;
    s.constitution.max_active_contracts = 0;
    const f = failing(s);
    expect(f.budget).toEqual(["roles hold 1100¢, constitution allows 1000¢"]);
    expect(f.authority).toEqual(["Scout holds ungranted scope payroll"]);
    expect(f.limits).toEqual(["2 live roles > limit 1", "1 open contracts > limit 0"]);
  });

  it("catches reporting to a retired role and cycles", () => {
    const s = base();
    s.roles!.b!.reports_to = "x";
    expect(failing(s).reporting).toEqual(["Scout reports to Retired lead, which is not live"]);
    const t = base();
    t.roles!.a!.reports_to = "b";
    expect(failing(t).reporting).toEqual(["Coordinator is in a reporting cycle", "Scout is in a reporting cycle"]);
  });

  it("catches executors that are gone", () => {
    const s = base();
    s.responsibilities!.d1!.executor = { kind: "role", id: "x" };
    s.responsibilities!.d2!.executor = { kind: "contractor", id: "nope" };
    expect(failing(s).executor).toEqual([
      "Coordinate is executed by Retired lead, not live",
      "Secure a venue names unknown contract nope",
    ]);
  });
});
