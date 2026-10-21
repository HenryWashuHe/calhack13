import { describe, expect, it } from "vitest";
import type { OrgState } from "./contracts";
import { historyView, SnapshotGate } from "./snapshot";

describe("SnapshotGate", () => {
  it("ignores an older request that completes after a newer one", () => {
    const gate = new SnapshotGate();
    const first = gate.begin();
    const second = gate.begin();
    expect(gate.accept(second, 2)).toBe(true);
    expect(gate.accept(first, 1)).toBe(false);
  });

  it("accepts an older request that completes first, then the newer one", () => {
    const gate = new SnapshotGate();
    const first = gate.begin();
    const second = gate.begin();
    expect(gate.accept(first, 1)).toBe(true);
    expect(gate.accept(second, 2)).toBe(true);
  });

  it("never accepts an older revision, even from a newer request", () => {
    const gate = new SnapshotGate();
    expect(gate.accept(gate.begin(), 3)).toBe(true);
    expect(gate.accept(gate.begin(), 2)).toBe(false);
    expect(gate.accept(gate.begin(), 3)).toBe(true);
  });

  it("reports errors only for the latest request", () => {
    const gate = new SnapshotGate();
    const first = gate.begin();
    const second = gate.begin();
    expect(gate.isLatest(first)).toBe(false);
    expect(gate.isLatest(second)).toBe(true);
  });

  it("starts over after reset", () => {
    const gate = new SnapshotGate();
    expect(gate.accept(gate.begin(), 9)).toBe(true);
    gate.reset();
    expect(gate.accept(gate.begin(), 1)).toBe(true);
  });
});

describe("historyView", () => {
  const r1 = { revision: 1 } as OrgState;

  it("is live without a selected revision", () => {
    expect(historyView(null, { revision: 1, state: r1 })).toEqual({ kind: "live" });
  });

  it("never shows another revision's snapshot", () => {
    expect(historyView(2, { revision: 1, state: r1 })).toEqual({ kind: "loading", revision: 2 });
  });

  it("reports a failed read for the requested revision", () => {
    expect(historyView(2, { revision: 2, error: "boom" })).toEqual({
      kind: "error",
      revision: 2,
      message: "boom",
    });
  });

  it("shows the snapshot for the requested revision", () => {
    expect(historyView(1, { revision: 1, state: r1 })).toEqual({ kind: "ready", revision: 1, state: r1 });
  });
});
