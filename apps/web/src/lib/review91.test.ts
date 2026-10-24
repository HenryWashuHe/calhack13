// Review of PR #91: inputs shaped like the real server's records on which the
// console says something false or misleading. Each test states the honest
// expectation; a failure is a defect.
import { describe, expect, it } from "vitest";
import { verdicts, type HistoryAudit } from "./audit";
import type { DomainEvent, OrgChangeRecord, OrgState, Role } from "./contracts";
import { generations, timeline } from "./evolution";
import { roleProvenance } from "./provenance";

let seq = 0;
function ev(revision: number, type: string, payload: Record<string, unknown>, mission_id?: string): DomainEvent {
  seq += 1;
  return {
    schema_version: 1,
    seq,
    event_id: `e${seq}`,
    org_id: "o",
    mission_id: mission_id ?? null,
    revision,
    timestamp: "",
    type,
    payload,
  } as unknown as DomainEvent;
}

const ROLE_C = "role_" + "c".repeat(32);
const ROLE_X = "role_" + "a".repeat(32);

function role(id: string, name: string, rev = 1, over: Partial<Role> = {}): Role {
  return {
    role_id: id,
    name,
    charter: "",
    authority: { scopes: [], budget_cents: 0 },
    reports_to: null,
    state: "ACTIVE",
    binding: null,
    capabilities: [],
    created_in_revision: rev,
    retired_in_revision: null,
    ...over,
  };
}

function baseState(over: Partial<OrgState> = {}): OrgState {
  return {
    org_id: "org_a",
    name: "A",
    revision: 10,
    constitution: { scopes: [], budget_cents: 0, max_active_roles: 9, max_active_contracts: 1 },
    missions: { m1: { mission_id: "m1", goal: "Run the hackathon", status: "ACTIVE", created_at: "" } },
    observations: {},
    requirements: {},
    responsibilities: {},
    roles: { [ROLE_C]: role(ROLE_C, "Coordinator") },
    ...over,
  } as unknown as OrgState;
}

function record(over: Partial<OrgChangeRecord> & { change_id: string }): OrgChangeRecord {
  return {
    status: "COMMITTED",
    proposal: { expected_org_revision: 9, trigger_evidence_ids: ["obs_1"], rationale: "r", operations: [] },
    allocation: {},
    submitted_by: "planner",
    created_at: "",
    updated_at: "",
    ...over,
  } as OrgChangeRecord;
}

describe("PR #91 review: changes that did not happen", () => {
  // genesis-planner agent.rs: a kernel rejection is recorded with
  // `PlannerVerdict::Rejected` and `change: None`, while saga.rs `submit`
  // stores the rejected submission as a REJECTED change record.
  it("a planner submission the kernel rejected is listed once, not twice", () => {
    const rejected = record({
      change_id: "chg_rej",
      status: "REJECTED",
      proposal: {
        expected_org_revision: 10,
        trigger_evidence_ids: ["obs_1"],
        rationale: "Create a second coordinator",
        operations: [{ op: "RetireRole", role_id: ROLE_C, handoffs: [] }],
      },
      errors: [{ code: "retirement_drops_obligations", message: `${ROLE_C} still owns work` }],
    } as Partial<OrgChangeRecord> & { change_id: string });
    const events = [
      ev(10, "change_proposed", { change_id: "chg_rej", expected_revision: 10, operations: ["RetireRole"], rationale: "" }),
      ev(10, "change_status_changed", { change_id: "chg_rej", from: "PROPOSED", to: "REJECTED" }),
      ev(10, "planner_decision", {
        planner: "llm",
        model: "claude-x",
        observation_id: "obs_1",
        verdict: "rejected",
        summary: "Create a second coordinator",
        rationale: "",
        attempt: 1,
        latency_ms: 5,
        change_id: null,
        kernel_errors: [`retirement_drops_obligations (operation 0): ${ROLE_C} still owns work`],
      }),
    ];
    const refusals = timeline(events, [rejected], baseState()).filter((t) => t.kind === "refusal");
    expect(refusals.map((r) => (r.kind === "refusal" ? r.title : ""))).toHaveLength(1);
  });

  it("planner-attempt kernel errors replace role IDs with names", () => {
    const events = [
      ev(10, "planner_decision", {
        planner: "llm",
        model: "claude-x",
        observation_id: "obs_1",
        verdict: "rejected",
        summary: "s",
        rationale: "",
        attempt: 1,
        latency_ms: 5,
        change_id: null,
        kernel_errors: [`role_not_active (operation 0): ${ROLE_C} is not active`],
      }),
    ];
    const [r] = timeline(events, [], baseState());
    expect(r!.kind === "refusal" ? r!.reasons.join(" ") : "").not.toContain(ROLE_C);
  });

  // saga.rs commit(): a non-conflict commit-time validation failure sets
  // record.errors and moves to FAILED, with no provisioning_failed event.
  // Provisioning had succeeded; the kernel refused at commit.
  it("a commit-time kernel refusal is not labeled a provisioning failure", () => {
    const failed = record({
      change_id: "chg_f",
      status: "FAILED",
      errors: [{ code: "budget_exceeded", message: "allocated budget would exceed the constitution" }],
      provisioned: { lead: { provider: "elastic", provider_agent_id: "agent-1", config_digest: "d" } },
      ready: ["lead"],
    } as Partial<OrgChangeRecord> & { change_id: string });
    const events = [
      ev(9, "change_proposed", { change_id: "chg_f", expected_revision: 9, operations: ["CreateRole"], rationale: "" }),
      ev(9, "role_provisioned", {
        change_id: "chg_f",
        role_key: "lead",
        role_id: ROLE_X,
        provider: "elastic",
        provider_agent_id: "agent-1",
        dev_stub: false,
      }),
      ev(10, "change_status_changed", { change_id: "chg_f", from: "READY", to: "FAILED" }),
    ];
    const [r] = timeline(events, [failed], baseState()).filter((t) => t.kind === "refusal");
    expect(r!.kind === "refusal" && r!.title).not.toMatch(/Provisioning failed/);
  });
});

describe("PR #91 review: operator-attached agents in Evolution", () => {
  // saga.rs attach_operation: no role_provisioned event; the operation keeps
  // native_status = "operator_attached". The binding provider is the real one.
  it("an operator-attached role is not shown in Evolution like a provider-created one", () => {
    const attached = record({
      change_id: "chg_a",
      committed_revision: 5,
      allocation: { roles: { monitor: ROLE_X } },
      provisioned: { monitor: { provider: "creao", provider_agent_id: "agent-op-7", config_digest: "d" } },
      operations: {
        monitor: {
          provider: "creao",
          operation_id: "op_1",
          state: "ready",
          native_status: "operator_attached",
          readiness_evidence: "receipt-1",
          updated_at: "",
        },
      },
    } as Partial<OrgChangeRecord> & { change_id: string });
    const events = [
      ev(1, "mission_created", { mission_id: "m1", goal: "g" }),
      ev(4, "provisioning_progress", {
        change_id: "chg_a",
        role_key: "monitor",
        provider: "creao",
        operation_id: "op_1",
        state: "awaiting_readiness",
        attention: "agent agent-op-7 attached by operator (found in console); readiness must still be proven",
      }),
      ev(5, "role_created", { role_id: ROLE_X, name: "Venue monitor", provider: "creao" }),
      ev(5, "role_activated", { role_id: ROLE_X }),
    ];
    const g = generations(events, [attached], baseState()).find((x) => x.revision === 5)!;
    const delta = g.deltas.find((d) => d.kind === "role_added")!;
    // Today: { kind: "role_added", text: "Venue monitor", provider: "creao", devStub: false },
    // rendered exactly like a CREAO agent the provider created.
    expect(JSON.stringify(delta)).toMatch(/operator/i);
  });
});

describe("PR #91 review: Why? drawer attribution", () => {
  // Two missions in one org (scripts/dev-failures.mjs adds a second mission to
  // org A). The coordinator was bootstrapped with m1, later inherited a duty
  // of m2. The drawer picks owned[0] by object key order.
  it("a bootstrap role is attributed to the mission that created it, not to key order", () => {
    const RSP_M1 = "rsp_" + "f".repeat(32);
    const RSP_M2 = "rsp_" + "0".repeat(32);
    const st = baseState({
      missions: {
        m1: { mission_id: "m1", goal: "Run the hackathon", status: "ACTIVE", created_at: "" },
        m2: { mission_id: "m2", goal: "Failure drills", status: "ACTIVE", created_at: "" },
      } as OrgState["missions"],
      responsibilities: {
        // serde serializes BTreeMap keys sorted: the m2 duty comes first.
        [RSP_M2]: { responsibility_id: RSP_M2, mission_id: "m2", purpose: "Run drills", owner: { kind: "role", id: ROLE_C }, trigger: "", termination: "", status: "OPEN", epoch: 2 },
        [RSP_M1]: { responsibility_id: RSP_M1, mission_id: "m1", purpose: "Coordinate", owner: { kind: "role", id: ROLE_C }, trigger: "", termination: "", status: "OPEN", epoch: 1 },
      } as OrgState["responsibilities"],
    });
    const events = [
      ev(1, "mission_created", { mission_id: "m1", goal: "Run the hackathon" }, "m1"),
      ev(1, "role_created", { role_id: ROLE_C, name: "Coordinator", provider: "internal" }, "m1"),
      ev(1, "role_activated", { role_id: ROLE_C }, "m1"),
      ev(6, "mission_created", { mission_id: "m2", goal: "Failure drills" }, "m2"),
    ];
    const p = roleProvenance(ROLE_C, st, [], events);
    expect(p.cause[0]!.detail).toBe("Run the hackathon");
  });

  // engine.rs RetireRole: a handoff to a role created in the same change emits
  // responsibility_transferred in the creating commit.
  it("a duty inherited at creation is not repeated as a later 'since then' event", () => {
    const RSP = "rsp_" + "1".repeat(32);
    const st = baseState({
      roles: {
        [ROLE_C]: role(ROLE_C, "Coordinator", 1, { state: "RETIRED", retired_in_revision: 5 }),
        [ROLE_X]: role(ROLE_X, "Successor", 5),
      },
      responsibilities: {
        [RSP]: { responsibility_id: RSP, mission_id: "m1", purpose: "Coordinate", owner: { kind: "role", id: ROLE_X }, trigger: "", termination: "", status: "OPEN", epoch: 2 },
      } as OrgState["responsibilities"],
    });
    const c = record({
      change_id: "chg_s",
      committed_revision: 5,
      allocation: { roles: { succ: ROLE_X } },
      proposal: {
        expected_org_revision: 4,
        trigger_evidence_ids: [],
        rationale: "Replace the coordinator",
        operations: [
          { op: "CreateRole", key: "succ", name: "Successor", charter: "", authority: { scopes: [], budget_cents: 0 }, provider: "internal", justification: "lifecycle" },
          { op: "RetireRole", role_id: ROLE_C, handoffs: [{ responsibility_id: RSP, to: { kind: "new", key: "succ" } }] },
        ],
      },
    } as Partial<OrgChangeRecord> & { change_id: string });
    const events = [
      ev(5, "role_created", { role_id: ROLE_X, name: "Successor", provider: "internal" }),
      ev(5, "role_activated", { role_id: ROLE_X }),
      ev(5, "responsibility_transferred", { responsibility_id: RSP, from: ROLE_C, to: ROLE_X, epoch: 2 }),
      ev(5, "role_draining", { role_id: ROLE_C }),
      ev(5, "role_retired", { role_id: ROLE_C }),
    ];
    const p = roleProvenance(ROLE_X, st, [c], events);
    expect(p.cause.some((s) => s.title.startsWith("Inherits a duty"))).toBe(true);
    expect(p.since.map((s) => s.title)).not.toContain("Took over a responsibility");
  });
});

describe("PR #91 review: Evolution labels", () => {
  it("a second mission added to a running organization is not a second founding", () => {
    const events = [
      ev(1, "mission_created", { mission_id: "m1", goal: "Run the hackathon" }),
      ev(1, "role_created", { role_id: ROLE_C, name: "Coordinator", provider: "internal" }),
      ev(1, "role_activated", { role_id: ROLE_C }),
      ev(6, "mission_created", { mission_id: "m2", goal: "Failure drills" }),
      ev(6, "role_created", { role_id: ROLE_X, name: "Drill lead", provider: "internal" }),
      ev(6, "role_activated", { role_id: ROLE_X }),
    ];
    const labels = generations(events, [], baseState()).map((g) => g.label);
    expect(labels.filter((l) => l === "Founded")).toHaveLength(1);
  });
});

describe("PR #91 review: kernel audit truncation", () => {
  // genesis-server audit.rs: snapshot violations are pushed in revision order,
  // then event-log violations (event_revision_known, event_org_matches) in seq
  // order, and only the first 200 are reported. When the list is truncated,
  // an unreported event-phase violation can sit at any revision, including
  // one below the highest reported revision.
  it("a truncated audit does not call an earlier revision clean", () => {
    const reported = Array.from({ length: 200 }, (_, i) => ({ revision: 10, invariant: "budget_limit", detail: `v${i}` }));
    const a: HistoryAudit = {
      org_id: "o",
      snapshots_checked: 10,
      first_revision: 1,
      last_revision: 10,
      events_checked: 50,
      invariants: ["budget_limit", "event_org_matches"],
      violation_count: 201, // the 201st is an event_org_matches violation at r3
      violations: reported,
    };
    const r3 = verdicts(a).find((v) => v.revision === 3)!;
    expect(r3.verdict).not.toBe("ok");
  });
});
