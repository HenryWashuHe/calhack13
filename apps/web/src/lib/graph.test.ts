import { describe as suite, expect, it } from "vitest";
import type { DomainEvent, OrgChangeRecord, OrgState, Role } from "./contracts";
import { buildTree, devStubTarget, pendingRoles } from "./graph";
import { describe } from "./describe";
import { mergeEvents } from "./useOrg";

function role(id: string, name: string, reports_to: string | null, state: Role["state"] = "ACTIVE"): Role {
  return {
    role_id: id,
    name,
    charter: "",
    authority: { scopes: [], budget_cents: 0 },
    reports_to,
    state,
    binding: null,
    capabilities: [],
    created_in_revision: 1,
    retired_in_revision: null,
  };
}

const state: OrgState = {
  org_id: "org_a",
  name: "A",
  revision: 3,
  constitution: { scopes: [], budget_cents: 0, max_active_roles: 5, max_active_contracts: 1 },
  roles: {
    role_c: role("role_c", "Coordinator", null),
    role_p: role("role_p", "Program", "role_c"),
    role_r: role("role_r", "Relocation", "role_c", "RETIRED"),
  },
};

suite("buildTree", () => {
  it("nests roles under their reporting line", () => {
    const tree = buildTree(state, [], { showRetired: true });
    expect(tree).toHaveLength(1);
    expect(tree[0]!.id).toBe("role_c");
    expect(tree[0]!.children.map((c) => c.id)).toEqual(["role_p", "role_r"]);
  });

  it("hides retired roles when asked", () => {
    const tree = buildTree(state, [], { showRetired: false });
    expect(tree[0]!.children.map((c) => c.id)).toEqual(["role_p"]);
  });
});

suite("pendingRoles", () => {
  const change = (status: OrgChangeRecord["status"]): OrgChangeRecord => ({
    change_id: "chg_1",
    status,
    proposal: {
      expected_org_revision: 3,
      trigger_evidence_ids: ["obs_1"],
      rationale: "",
      budget_limit_cents: null,
      operations: [
        {
          op: "CreateRole",
          key: "monitor",
          name: "Venue monitor",
          charter: "Watch",
          authority: { scopes: [] },
          reports_to: { kind: "existing", role_id: "role_c" },
          provider: "creao",
          justification: "lifecycle",
        },
      ],
    },
    allocation: { roles: { monitor: "role_m" } },
    provisioned: {},
    errors: [],
    committed_revision: null,
    submitted_by: "agent",
    created_at: "",
    updated_at: "",
  });

  it("shows in-flight roles as pending under their parent", () => {
    const pending = pendingRoles([change("PROVISIONING")], state);
    expect(pending).toHaveLength(1);
    const tree = buildTree(state, pending, { showRetired: false });
    const monitor = tree[0]!.children.find((c) => c.id === "role_m");
    expect(monitor?.kind).toBe("pending");
  });

  it("carries the asynchronous provider operation of a pending role", () => {
    const c = change("PROVISIONING");
    c.operations = {
      monitor: {
        provider: "creao",
        operation_id: "creao-role-1",
        state: "awaiting_readiness",
        run_id: "run-1",
        native_status: "completed",
        attention: "no readiness verifier",
        readiness_evidence: null,
        updated_at: "",
      },
    };
    const [p] = pendingRoles([c], state);
    expect(p!.operation?.state).toBe("awaiting_readiness");
    expect(pendingRoles([change("PROVISIONING")], state)[0]!.operation).toBeNull();
  });

  it("never shows failed or rejected changes as roles", () => {
    expect(pendingRoles([change("FAILED"), change("REJECTED"), change("COMMITTED")], state)).toHaveLength(0);
  });
});

suite("dev stub labeling", () => {
  it("extracts the provider a stub stands in for", () => {
    expect(
      devStubTarget({ provider: "dev_stub", provider_agent_id: "devstub-elastic-abc", config_digest: "" }),
    ).toBe("elastic");
    expect(devStubTarget({ provider: "elastic", provider_agent_id: "agent-1", config_digest: "" })).toBeNull();
  });

  it("marks dev-stub provisioning events", () => {
    const ev: DomainEvent = {
      schema_version: 1,
      seq: 5,
      event_id: "evt_1",
      org_id: "org_a",
      revision: 3,
      timestamp: new Date().toISOString(),
      type: "role_provisioned",
      payload: {
        change_id: "chg_1",
        role_key: "relocation",
        role_id: "role_x",
        provider: "elastic",
        provider_agent_id: "devstub-elastic-1",
        dev_stub: true,
      },
    };
    const d = describe(ev, state);
    expect(d.devStub).toBe(true);
    expect(d.title).toContain("dev stub");
  });
});

suite("mergeEvents", () => {
  it("dedupes by sequence and keeps order", () => {
    const mk = (seq: number) =>
      ({ seq, type: "role_activated", payload: { role_id: "r" } }) as unknown as DomainEvent;
    const merged = mergeEvents([mk(1), mk(3)], [mk(2), mk(3)]);
    expect(merged.map((e) => e.seq)).toEqual([1, 2, 3]);
  });
});
