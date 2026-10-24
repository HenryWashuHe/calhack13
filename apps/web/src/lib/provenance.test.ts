import { describe as suite, expect, it } from "vitest";
import type { DomainEvent, OrgChangeRecord, OrgState, Role } from "./contracts";
import { creatingChange, roleProvenance } from "./provenance";

function role(id: string, name: string, state: Role["state"] = "ACTIVE", rev = 1): Role {
  return {
    role_id: id,
    name,
    charter: "",
    authority: { scopes: [], budget_cents: 0 },
    reports_to: null,
    state,
    binding: null,
    capabilities: [],
    created_in_revision: rev,
    retired_in_revision: null,
  };
}

const state: OrgState = {
  org_id: "org_a",
  name: "A",
  revision: 5,
  constitution: { scopes: [], budget_cents: 0, max_active_roles: 5, max_active_contracts: 1 },
  missions: {
    m1: { mission_id: "m1", goal: "Run the hackathon", status: "ACTIVE", created_at: "" },
  },
  observations: {
    obs_1: {
      observation_id: "obs_1",
      mission_id: "m1",
      source: "venue-portal",
      summary: "Venue cancelled",
      evidence_ref: "portal://notice/7",
      observed_at: "",
    },
  },
  requirements: {
    req_1: {
      requirement_id: "req_1",
      mission_id: "m1",
      summary: "Find a replacement venue",
      triggering_evidence: ["obs_1"],
      acceptance: "a venue is reserved",
      status: "ASSIGNED",
      owner: { kind: "role", id: "role_v" },
    },
  },
  responsibilities: {
    resp_c: {
      responsibility_id: "resp_c",
      mission_id: "m1",
      purpose: "Coordinate",
      owner: { kind: "role", id: "role_c" },
      trigger: "",
      termination: "",
      status: "OPEN",
      epoch: 1,
    },
    resp_v: {
      responsibility_id: "resp_v",
      mission_id: "m1",
      purpose: "Secure a venue",
      requirement_id: "req_1",
      owner: { kind: "role", id: "role_v" },
      trigger: "",
      termination: "",
      status: "OPEN",
      epoch: 2,
    },
  },
  roles: {
    role_c: role("role_c", "Coordinator"),
    role_v: role("role_v", "Venue scout", "ACTIVE", 4),
  },
};

function change(over: Partial<OrgChangeRecord> = {}): OrgChangeRecord {
  return {
    change_id: "chg_1",
    status: "COMMITTED",
    proposal: {
      expected_org_revision: 3,
      trigger_evidence_ids: ["obs_1"],
      rationale: "The venue fell through; a dedicated scout owns the search.",
      budget_limit_cents: null,
      operations: [
        {
          op: "CreateRole",
          key: "scout",
          name: "Venue scout",
          charter: "Find venues",
          authority: { scopes: [] },
          provider: "creao",
          justification: "independent_ownership",
        },
        {
          op: "AssignResponsibility",
          target: { kind: "from_requirement", key: "venue", requirement_id: "req_1", purpose: "Secure a venue", trigger: "requirement", termination: "venue reserved" },
          owner: { kind: "new", key: "scout" },
        },
      ],
    },
    allocation: { roles: { scout: "role_v" }, responsibilities: { venue: "resp_v" } },
    provisioned: {
      scout: { provider: "creao", provider_agent_id: "agent-42", config_digest: "sha256:abc" },
    },
    operations: {
      scout: {
        provider: "creao",
        operation_id: "op-1",
        state: "ready",
        run_id: "run-9",
        native_status: "completed",
        readiness_evidence: "readiness:rcpt-1",
        updated_at: "",
      },
    },
    errors: [],
    committed_revision: 4,
    submitted_by: "planner",
    created_at: "",
    updated_at: "",
    ...over,
  };
}

let seq = 0;
function ev(e: Pick<DomainEvent, "type" | "payload"> & { revision?: number }): DomainEvent {
  seq += 1;
  return {
    schema_version: 1,
    seq,
    event_id: `evt_${seq}`,
    org_id: "org_a",
    revision: e.revision ?? 4,
    timestamp: "",
    type: e.type,
    payload: e.payload,
  } as DomainEvent;
}

const decision = ev({
  type: "planner_decision",
  payload: {
    planner: "llm",
    model: "claude-test",
    observation_id: "obs_1",
    verdict: "change_proposed",
    summary: "Hire a venue scout",
    rationale: "No existing role owns venue search",
    attempt: 1,
    latency_ms: 812,
    requirement_id: "req_1",
    change_id: "chg_1",
  },
} as Pick<DomainEvent, "type" | "payload">);

suite("creatingChange", () => {
  it("finds the committed change that allocated the role", () => {
    expect(creatingChange("role_v", [change()])?.key).toBe("scout");
  });

  it("ignores changes that never committed", () => {
    expect(creatingChange("role_v", [change({ status: "FAILED" }), change({ status: "PROVISIONING" })])).toBeNull();
  });
});

suite("roleProvenance", () => {
  it("chains observation, requirement, planner, change, provisioning, readiness and commit", () => {
    const p = roleProvenance("role_v", state, [change()], [decision]);
    expect(p.origin).toBe("change");
    expect(p.changeId).toBe("chg_1");
    expect(p.cause.map((s) => s.kind)).toEqual([
      "observation",
      "requirement",
      "planner",
      "change",
      "provisioning",
      "readiness",
      "commit",
    ]);
    expect(p.cause[0]!.refs).toEqual(["obs_1", "portal://notice/7"]);
    expect(p.cause[1]!.detail).toContain("a venue is reserved");
    expect(p.cause[2]!.title).toContain("claude-test");
    expect(p.cause[3]!.detail).toContain("independent ownership");
    expect(p.cause[4]!.title).toBe("Agent created on creao");
    expect(p.cause[4]!.devStub).toBeFalsy();
    expect(p.cause[6]!.title).toBe("Committed in r4");
  });

  it("labels development stubs and operator attachments, never as live creation", () => {
    const stub = change({
      provisioned: {
        scout: { provider: "dev_stub", provider_agent_id: "devstub-creao-1", config_digest: "" },
      },
      operations: {},
    });
    const s = roleProvenance("role_v", state, [stub], []).cause.find((x) => x.kind === "provisioning")!;
    expect(s.devStub).toBe(true);
    expect(s.title).toContain("development stub standing in for creao");
    expect(s.tone).toBe("warn");

    const attached = change();
    attached.operations!.scout!.native_status = "operator_attached";
    const a = roleProvenance("role_v", state, [attached], []).cause.find((x) => x.kind === "provisioning")!;
    expect(a.title).toContain("attached by an operator");
    expect(a.devStub).toBe(false);
    expect(a.tone).toBe("warn");
  });

  it("does not invent a planner step when the change was submitted by hand", () => {
    const p = roleProvenance("role_v", state, [change({ submitted_by: "operator" })], []);
    expect(p.cause.some((s) => s.kind === "planner")).toBe(false);
    expect(p.cause.find((s) => s.kind === "change")!.title).toContain("operator");
  });

  it("attributes bootstrap roles to their mission", () => {
    const p = roleProvenance("role_c", state, [change()], []);
    expect(p.origin).toBe("mission");
    expect(p.cause).toHaveLength(1);
    expect(p.cause[0]!.detail).toBe("Run the hackathon");
    expect(p.cause[0]!.refs).toEqual(["m1"]);
  });

  it("records ownership moves, capabilities and retirement afterwards", () => {
    const retire = change({
      change_id: "chg_2",
      proposal: {
        expected_org_revision: 5,
        trigger_evidence_ids: ["obs_1"],
        rationale: "Venue secured; scout no longer needed.",
        operations: [
          { op: "RetireRole", role_id: "role_v", handoffs: [{ responsibility_id: "resp_v", to: { kind: "existing", role_id: "role_c" } }] },
        ],
      },
      allocation: {},
      provisioned: {},
      operations: {},
      committed_revision: 6,
    });
    const events = [
      ev({ type: "capability_attached", payload: { role_id: "role_v", capability: { capability_id: "venue.search", version: "1.0.0" } } } as Pick<DomainEvent, "type" | "payload">),
      ev({ type: "responsibility_transferred", payload: { responsibility_id: "resp_v", from: "role_v", to: "role_c", epoch: 3 } } as Pick<DomainEvent, "type" | "payload">),
      ev({ type: "role_retired", payload: { role_id: "role_v" }, revision: 6 } as Pick<DomainEvent, "type" | "payload"> & { revision: number }),
      ev({ type: "role_retired", payload: { role_id: "role_other" } } as Pick<DomainEvent, "type" | "payload">),
    ];
    const p = roleProvenance("role_v", state, [change(), retire], events);
    expect(p.since.map((s) => s.kind)).toEqual(["capability", "ownership", "retirement"]);
    expect(p.since[0]!.detail).toBe("venue.search@1.0.0");
    expect(p.since[1]!.title).toBe("Handed off a responsibility");
    expect(p.since[1]!.detail).toContain("Secure a venue");
    expect(p.since[1]!.detail).toContain("Coordinator");
    expect(p.since[2]!.title).toBe("Retired in r6");
    expect(p.since[2]!.refs).toEqual(["chg_2"]);

    const c = roleProvenance("role_c", state, [change(), retire], events);
    expect(c.since.map((s) => s.title)).toEqual(["Took over a responsibility"]);
  });

  it("shows duties a new role inherits from a retiring one", () => {
    const succession = change({
      change_id: "chg_3",
      proposal: {
        expected_org_revision: 5,
        trigger_evidence_ids: ["obs_1"],
        rationale: "Monitoring continues after relocation.",
        operations: [
          {
            op: "CreateRole",
            key: "monitor",
            name: "Venue monitor",
            charter: "Watch",
            authority: { scopes: [] },
            provider: "creao",
            justification: "lifecycle",
          },
          { op: "RetireRole", role_id: "role_v", handoffs: [{ responsibility_id: "resp_v", to: { kind: "new", key: "monitor" } }] },
        ],
      },
      allocation: { roles: { monitor: "role_m" } },
      provisioned: {},
      operations: {},
      committed_revision: 6,
    });
    const p = roleProvenance("role_m", state, [change(), succession], []);
    const inherit = p.cause.find((s) => s.kind === "ownership")!;
    expect(inherit.title).toBe("Inherits a duty from Venue scout");
    expect(inherit.detail).toBe("Secure a venue");
    expect(inherit.refs).toEqual(["resp_v"]);
  });

  it("is honest about roles it cannot explain", () => {
    const p = roleProvenance("role_missing", state, [], []);
    expect(p.origin).toBe("unknown");
    expect(p.cause).toEqual([]);
  });
});
