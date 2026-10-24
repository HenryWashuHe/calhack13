import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Api } from "../lib/api";
import type { OrgState } from "../lib/contracts";
import { AuditStrip } from "./AuditStrip";

const state: OrgState = {
  org_id: "o",
  name: "O",
  revision: 2,
  constitution: { scopes: [], budget_cents: 100, max_active_roles: 1, max_active_contracts: 0 },
  roles: {
    a: { role_id: "a", name: "A", charter: "", authority: { budget_cents: 50 }, state: "ACTIVE", created_in_revision: 1 },
    b: { role_id: "b", name: "B", charter: "", authority: { budget_cents: 80 }, state: "ACTIVE", created_in_revision: 2 },
  },
};

describe("AuditStrip", () => {
  it("summarizes failing rules at the shown revision", () => {
    const html = renderToStaticMarkup(<AuditStrip api={{} as Api} state={state} current={2} onSelect={() => {}} />);
    expect(html).toContain("6/8 rules hold at r2");
    expect(html).toContain("audit bad");
    expect(html).toContain("Audit all 3 revisions");
  });
});
