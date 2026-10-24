import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { HistoryAudit } from "../lib/audit";
import { AuditView } from "./AuditStrip";

const clean: HistoryAudit = {
  org_id: "org_a",
  snapshots_checked: 3,
  first_revision: 0,
  last_revision: 2,
  events_checked: 9,
  invariants: ["role_limit", "budget_limit", "live_owner"],
  violation_count: 0,
  violations: [],
};

const noop = () => {};

describe("AuditView", () => {
  it("states what the kernel checked when history is clean", () => {
    const html = renderToStaticMarkup(<AuditView audit={clean} shown={2} current={2} onSelect={noop} open onToggle={noop} />);
    expect(html).toContain("every revision r0–r2 passes 3 invariants");
    expect(html).toContain("3 snapshots · 9 events");
    expect(html).toContain("budget limit");
    expect(html).toContain("audit-cell ok here");
  });

  it("names violations on the revision shown", () => {
    const bad = { ...clean, violation_count: 1, violations: [{ revision: 1, invariant: "budget_limit", detail: "live roles hold 120 cents" }] };
    const html = renderToStaticMarkup(<AuditView audit={bad} shown={1} current={2} onSelect={noop} open onToggle={noop} />);
    expect(html).toContain("1 violation in stored history");
    expect(html).toContain("audit bad");
    expect(html).toContain("r1 budget limit: live roles hold 120 cents");
    expect(html).toContain("audit-cell bad here");
  });
});
