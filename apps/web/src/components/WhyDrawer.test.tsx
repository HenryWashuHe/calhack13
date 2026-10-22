import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { OrgChangeRecord, OrgState } from "../lib/contracts";
import { WhyDrawer } from "./WhyDrawer";

const state: OrgState = {
  org_id: "o",
  name: "O",
  revision: 2,
  constitution: { scopes: [], budget_cents: 0, max_active_roles: 3, max_active_contracts: 0 },
  observations: {
    obs: { observation_id: "obs", mission_id: "m", source: "operator", summary: "Speaker dropped out", observed_at: "" },
  },
  roles: {
    r: {
      role_id: "r",
      name: "Speaker wrangler",
      charter: "",
      authority: {},
      state: "ACTIVE",
      created_in_revision: 2,
    },
  },
};

const stubChange: OrgChangeRecord = {
  change_id: "chg",
  status: "COMMITTED",
  proposal: { expected_org_revision: 1, trigger_evidence_ids: ["obs"], rationale: "Need a replacement speaker", operations: [] },
  allocation: { roles: { w: "r" } },
  provisioned: { w: { provider: "dev_stub", provider_agent_id: "devstub-elastic-9", config_digest: "d" } },
  committed_revision: 2,
  submitted_by: "planner",
  created_at: "",
  updated_at: "",
};

describe("WhyDrawer", () => {
  it("renders the cause chain and flags dev stubs as not live", () => {
    const html = renderToStaticMarkup(<WhyDrawer roleId="r" state={state} changes={[stubChange]} events={[]} />);
    expect(html).toContain("Speaker dropped out");
    expect(html).toContain("Need a replacement speaker");
    expect(html).toContain("standing in for elastic");
    expect(html).toContain("dev stub, not live");
    expect(html).toContain("Committed in r2");
  });

  it("says so when nothing explains the role", () => {
    const html = renderToStaticMarkup(<WhyDrawer roleId="ghost" state={state} changes={[]} events={[]} />);
    expect(html).toContain("No recorded cause");
  });
});
