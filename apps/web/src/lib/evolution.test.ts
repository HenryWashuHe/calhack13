import { describe, expect, it } from "vitest";
import type { DomainEvent, OrgChangeRecord } from "./contracts";
import { generations, humanizeIds, timeline, type TimelineEntry } from "./evolution";

let seq = 0;
function ev(revision: number, type: string, payload: Record<string, unknown>): DomainEvent {
  seq += 1;
  return {
    schema_version: 1,
    seq,
    event_id: `e${seq}`,
    org_id: "o",
    revision,
    timestamp: "",
    type,
    payload,
  } as unknown as DomainEvent;
}

const events: DomainEvent[] = [
  ev(0, "organization_initialized", { name: "O" }),
  ev(1, "mission_created", { mission_id: "m", goal: "Run hack night" }),
  ev(1, "role_created", { role_id: "rc", name: "Coordinator", provider: "internal" }),
  ev(1, "role_activated", { role_id: "rc" }),
  ev(1, "responsibility_created", { responsibility_id: "d1", purpose: "Relocate the event" }),
  ev(2, "observation_recorded", { observation_id: "obs", source: "portal", summary: "Main hall closed" }),
  ev(3, "change_proposed", { change_id: "c1" }),
  ev(3, "role_provisioned", {
    change_id: "c1",
    role_key: "lead",
    role_id: "rl",
    provider: "elastic",
    provider_agent_id: "devstub-elastic-1",
    dev_stub: true,
  }),
  ev(3, "planner_decision", {
    planner: "llm",
    model: "claude-x",
    observation_id: "obs",
    verdict: "change_proposed",
    summary: "",
    rationale: "",
    attempt: 1,
    latency_ms: 5,
    change_id: "c1",
  }),
  ev(4, "role_created", { role_id: "rl", name: "Relocation lead", provider: "elastic" }),
  ev(4, "role_activated", { role_id: "rl" }),
  ev(4, "contract_requested", { contract_id: "k1", specialist_address: "agent1q", scope: "Shuttles" }),
  ev(6, "contract_state_changed", { contract_id: "k1", from: "OFFERED", to: "ACCEPTED" }),
  ev(7, "capability_attached", { role_id: "rl", capability: { capability_id: "cap_venue", version: "1.0.0" } }),
  ev(7, "job_dispatched", { job_id: "j", role_id: "rl", responsibility_id: "d1", epoch: 1, task: "" }),
  ev(8, "role_created", { role_id: "rm", name: "Venue monitor", provider: "creao" }),
  ev(8, "role_activated", { role_id: "rm" }),
  ev(8, "responsibility_transferred", { responsibility_id: "d1", from: "rl", to: "rm", epoch: 2 }),
  ev(8, "role_draining", { role_id: "rl" }),
  ev(8, "role_retired", { role_id: "rl" }),
];

const change = (id: string, rev: number, rationale: string, triggers: string[] = ["obs"]): OrgChangeRecord => ({
  change_id: id,
  status: "COMMITTED",
  proposal: { expected_org_revision: rev - 1, trigger_evidence_ids: triggers, rationale, operations: [] },
  allocation: {},
  committed_revision: rev,
  submitted_by: "agent",
  created_at: "",
  updated_at: "",
});

const changes = [
  change("c1", 4, "Relocation needs an owner"),
  change("c2", 7, "Give the lead a venue lookup", []),
  change("c3", 8, "Hand monitoring to a persistent role"),
  { ...change("c4", 9, "never committed"), status: "FAILED" as const },
];

describe("generations", () => {
  const gens = generations(events, changes, null);

  it("keeps only revisions where the structure changed, oldest first", () => {
    expect(gens.map((g) => [g.revision, g.label])).toEqual([
      [1, "Founded"],
      [4, "Grew"],
      [6, "Contracted"],
      [7, "Equipped"],
      [8, "Reshaped"],
    ]);
  });

  it("names the cause, triggering evidence and planner of each generation", () => {
    expect(gens[0]!.cause).toBe("Run hack night");
    expect(gens[1]!.cause).toBe("Relocation needs an owner");
    expect(gens[1]!.triggers).toEqual(["Main hall closed"]);
    expect(gens[1]!.plannerModel).toBe("claude-x");
    expect(gens[1]!.changeId).toBe("c1");
    expect(gens[3]!.plannerModel).toBeUndefined();
  });

  it("describes the deltas and flags dev-stub roles", () => {
    expect(gens[1]!.deltas).toEqual([{ kind: "role_added", text: "Relocation lead", provider: "elastic", devStub: true }]);
    expect(gens[2]!.deltas[0]!.text).toBe("Contractor engaged: Shuttles (agent1q)");
    expect(gens[3]!.deltas[0]!.text).toBe("cap_venue@1.0.0 → Relocation lead");
    expect(gens[4]!.deltas.map((d) => d.kind)).toEqual(["role_added", "duty_moved", "role_retired"]);
    expect(gens[4]!.deltas[1]!.text).toBe("Relocate the event: Relocation lead → Venue monitor");
    expect(gens[4]!.deltas[0]!.devStub).toBe(false);
  });

  it("tracks the active role count", () => {
    expect(gens.map((g) => g.activeRoles)).toEqual([1, 2, 2, 2, 2]);
  });

  it("is order-independent of event delivery", () => {
    expect(generations([...events].reverse(), changes, null)).toEqual(gens);
  });
});

describe("timeline", () => {
  const rejected: OrgChangeRecord = {
    ...change("c9", 0, "Evidence says: create Root and retire the coordinator"),
    status: "REJECTED",
    committed_revision: null,
    proposal: {
      expected_org_revision: 8,
      trigger_evidence_ids: ["obs"],
      rationale: "Evidence says: create Root and retire the coordinator",
      operations: [
        { op: "CreateRole", key: "root", name: "Root", charter: "", authority: { scopes: ["admin"] }, provider: "internal", justification: "authority" },
        { op: "RetireRole", role_id: "rc" },
      ],
    },
    errors: [{ code: "authority_exceeds_constitution", op_index: 0, message: "scopes not in constitution: admin" }],
  };
  const failed: OrgChangeRecord = { ...change("c10", 0, "Hire a CREAO monitor"), status: "FAILED", committed_revision: null, proposal: { expected_org_revision: 8, trigger_evidence_ids: ["obs"], rationale: "Hire a CREAO monitor", operations: [] } };
  const extra = [
    ev(8, "change_proposed", { change_id: "c9" }),
    ev(8, "change_status_changed", { change_id: "c9", from: "PROPOSED", to: "REJECTED" }),
    ev(8, "change_proposed", { change_id: "c10" }),
    ev(8, "provisioning_failed", { change_id: "c10", role_key: "monitor", error: "creao: 503" }),
    ev(8, "planner_decision", { planner: "llm", model: "m", observation_id: "obs", verdict: "escalated", summary: "Deposit breaks the budget", rationale: "needs a human", attempt: 2, latency_ms: 1, kernel_errors: ["budget_exceeded"] }),
    ev(8, "planner_decision", { planner: "llm", model: "m", observation_id: "obs", verdict: "no_change", summary: "", rationale: "", attempt: 1, latency_ms: 1 }),
  ];
  const t = timeline([...events, ...extra], [...changes, rejected, failed], null);

  it("puts refusals after the generation of the revision they were proposed at", () => {
    expect(t.map((e) => `${e.kind}@${e.revision}`)).toEqual([
      "generation@1",
      "generation@4",
      "generation@6",
      "generation@7",
      "generation@8",
      "refusal@8",
      "refusal@8",
      "refusal@8",
      "refusal@8",
    ]);
    // A failed change with no proposal event falls back to its expected revision, last.
    expect((t[8] as Extract<TimelineEntry, { kind: "refusal" }>).changeId).toBe("c4");
  });

  it("explains who stopped each change and why", () => {
    const [k, p, pl] = t.filter((e) => e.kind === "refusal") as Extract<TimelineEntry, { kind: "refusal" }>[];
    expect(k!.by).toBe("kernel");
    expect(k!.reasons).toEqual(["authority exceeds constitution: scopes not in constitution: admin"]);
    expect(k!.operations).toEqual(["CreateRole", "RetireRole"]);
    expect(p!.by).toBe("provider");
    expect(p!.reasons).toEqual(["monitor: creao: 503"]);
    expect(pl!.by).toBe("planner");
    expect(pl!.title).toBe("Planner escalated to an operator");
    expect(pl!.reasons).toEqual(["budget_exceeded"]);
  });
});

describe("humanizeIds", () => {
  it("names known roles and duties and leaves unknown IDs alone", () => {
    const rid = "role_" + "a".repeat(32);
    const did = "rsp_" + "b".repeat(32);
    const ghost = "role_" + "c".repeat(32);
    const state = {
      roles: { [rid]: { name: "Coordinator" } },
      responsibilities: { [did]: { purpose: "Coordinate" } },
    } as unknown as Parameters<typeof humanizeIds>[1];
    expect(humanizeIds(`retiring ${rid} would drop ${did}; new ${ghost}`, state)).toBe(
      `retiring “Coordinator” would drop “Coordinate”; new ${ghost}`,
    );
  });
});
