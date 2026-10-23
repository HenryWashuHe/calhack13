import { describe, expect, it } from "vitest";
import { type HistoryAudit, ruleName, verdicts } from "./audit";

const base: HistoryAudit = {
  org_id: "org_a",
  snapshots_checked: 4,
  first_revision: 0,
  last_revision: 3,
  events_checked: 20,
  invariants: ["role_limit", "budget_limit"],
  violation_count: 0,
  violations: [],
};

describe("verdicts", () => {
  it("marks every revision ok when the history is clean", () => {
    expect(verdicts(base).map((r) => r.verdict)).toEqual(["ok", "ok", "ok", "ok"]);
  });

  it("attributes violations to their revision", () => {
    const a = { ...base, violation_count: 1, violations: [{ revision: 2, invariant: "budget_limit", detail: "x" }] };
    expect(verdicts(a).map((r) => r.verdict)).toEqual(["ok", "ok", "bad", "ok"]);
  });

  it("calls no unflagged revision clean when the list is truncated", () => {
    const a = { ...base, violation_count: 500, violations: [{ revision: 1, invariant: "role_limit", detail: "x" }] };
    expect(verdicts(a).map((r) => r.verdict)).toEqual(["unknown", "bad", "unknown", "unknown"]);
  });

  it("handles an empty history", () => {
    expect(verdicts({ ...base, first_revision: null, last_revision: null, snapshots_checked: 0 })).toEqual([]);
  });

  it("reads rule names", () => {
    expect(ruleName("single_active_assignment")).toBe("single active assignment");
  });
});
