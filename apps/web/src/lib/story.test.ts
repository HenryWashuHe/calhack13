import { describe, expect, it } from "vitest";
import type { DomainEvent, OrgChangeRecord, OrgState } from "./contracts";
import { storyHtml } from "./story";

const state: OrgState = {
  org_id: "org_a",
  name: "Org <A> & Co",
  revision: 2,
  constitution: { scopes: [], budget_cents: 100, max_active_roles: 3, max_active_contracts: 0 },
  observations: {
    obs: { observation_id: "obs", mission_id: "m", source: "portal", summary: "<script>alert(1)</script>", observed_at: "" },
  },
  roles: {
    r: {
      role_id: "r",
      name: "Scout",
      charter: "",
      authority: {},
      state: "ACTIVE",
      binding: { provider: "dev_stub", provider_agent_id: "devstub-elastic-1", config_digest: "" },
      created_in_revision: 2,
    },
  },
};
const change: OrgChangeRecord = {
  change_id: "chg",
  status: "COMMITTED",
  proposal: { expected_org_revision: 1, trigger_evidence_ids: ["obs"], rationale: "Need a scout", operations: [] },
  allocation: { roles: { s: "r" } },
  provisioned: { s: { provider: "dev_stub", provider_agent_id: "devstub-elastic-1", config_digest: "" } },
  committed_revision: 2,
  submitted_by: "agent",
  created_at: "",
  updated_at: "",
};
const events = [
  { schema_version: 1, seq: 1, event_id: "e1", org_id: "org_a", revision: 2, timestamp: "", type: "role_created", payload: { role_id: "r", name: "Scout", provider: "elastic" } },
  { schema_version: 1, seq: 2, event_id: "e2", org_id: "org_a", revision: 2, timestamp: "", type: "role_activated", payload: { role_id: "r" } },
] as DomainEvent[];

describe("storyHtml", () => {
  const audit = {
    org_id: "org_a",
    snapshots_checked: 3,
    first_revision: 0,
    last_revision: 2,
    events_checked: 2,
    invariants: ["role_limit", "budget_limit"],
    violation_count: 0,
    violations: [],
  };
  const html = storyHtml(state, [change], events, audit, new Date("2026-10-22T00:00:00Z"));

  it("is a standalone page with evolution, audit and per-role causes", () => {
    expect(html.startsWith("<!doctype html>")).toBe(true);
    expect(html).toContain("<h2>Evolution</h2>");
    expect(html).toContain("Kernel audit: every stored revision r0–r2 passes 2 invariants");
    expect(storyHtml(state, [change], events, null)).toContain("kernel audit was not available");
    expect(html).toContain("Why each role exists");
    expect(html).toContain("Need a scout");
    expect(html).toContain("2026-10-22T00:00:00.000Z");
  });

  it("labels development stubs and escapes recorded text", () => {
    expect(html).toContain("Development stubs stood in");
    expect(html).toContain("dev stub, not live");
    expect(html).not.toContain("<script>alert(1)</script>");
    expect(html).toContain("&lt;script&gt;alert(1)&lt;/script&gt;");
    expect(html).toContain("Org &lt;A&gt; &amp; Co");
  });
});
