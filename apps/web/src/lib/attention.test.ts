import { describe as suite, expect, it } from "vitest";
import type { DomainEvent, Job, OrgChangeRecord } from "./contracts";
import { attention, counts, isOpen, validAgentId } from "./attention";

function ev(seq: number, type: string, payload: Record<string, unknown>): DomainEvent {
  return {
    schema_version: 1,
    seq,
    event_id: `ev_${seq}`,
    org_id: "org_a",
    revision: 1,
    timestamp: "2026-10-22T00:00:00Z",
    type,
    payload,
  } as unknown as DomainEvent;
}

function change(id: string, status: OrgChangeRecord["status"], operations: Record<string, unknown>): OrgChangeRecord {
  return { change_id: id, status, operations } as unknown as OrgChangeRecord;
}

function job(id: string, status: Job["status"], extra: Partial<Job> = {}): Job {
  return { job_id: id, status, task: `task ${id}`, role_id: "role_x", ...extra } as unknown as Job;
}

const op = (state: string, extra: Record<string, unknown> = {}) => ({
  provider: "creao",
  operation_id: "creao-role-1",
  state,
  updated_at: "2026-10-22T00:00:00Z",
  ...extra,
});

suite("attention", () => {
  it("asks the operator to reconcile a lost provider outcome", () => {
    const items = attention(
      [change("chg_1", "PROVISIONING", { monitor: op("needs_reconciliation", { run_id: "run-9" }) })],
      [],
      [],
      null,
    );
    expect(items).toHaveLength(1);
    expect(items[0]!.kind).toBe("action");
    expect(items[0]!.reconcile).toEqual({
      change_id: "chg_1",
      role_key: "monitor",
      provider: "creao",
      operation_id: "creao-role-1",
      run_id: "run-9",
    });
    expect(items[0]!.detail).toContain("run run-9");
  });

  it("ignores operations of changes that are no longer provisioning", () => {
    const items = attention(
      [change("chg_1", "FAILED", { monitor: op("needs_reconciliation") }), change("chg_2", "COMMITTED", { m: op("ready") })],
      [],
      [],
      null,
    );
    expect(items).toEqual([]);
  });

  it("shows created agents waiting for readiness, with the reason", () => {
    const items = attention(
      [change("chg_1", "PROVISIONING", { monitor: op("awaiting_readiness", { attention: "no readiness verifier" }) })],
      [],
      [],
      null,
    );
    expect(items[0]!.kind).toBe("waiting");
    expect(items[0]!.detail).toContain("no readiness verifier");
    expect(items[0]!.reconcile).toBeUndefined();
  });

  it("separates unknown outcomes and unverified provider success from successes", () => {
    const items = attention([], [job("job_u", "UNKNOWN"), job("job_v", "VERIFYING"), job("job_ok", "SUCCEEDED")], [], null);
    expect(items.map((i) => [i.kind, i.refs[0]])).toEqual([
      ["unknown", "job_u"],
      ["waiting", "job_v"],
    ]);
  });

  it("lists refusals and failures newest first, after open items", () => {
    const events = [
      ev(10, "change_status_changed", { change_id: "chg_a", from: "PROPOSED", to: "REJECTED", reasons: ["RevisionConflict: stale"] }),
      ev(11, "job_result_rejected", { job_id: "job_1", reason: "stale epoch: current 2, got 1" }),
      ev(12, "provisioning_failed", { change_id: "chg_b", role_key: "r", error: "boom", provider_agent_id: "agent-7" }),
      ev(13, "change_status_changed", { change_id: "chg_b", from: "PROVISIONING", to: "FAILED", reasons: ["provisioning `r` failed: boom"] }),
      ev(14, "installation_rejected", { installation_id: "inst_1", reasons: ["fixture mismatch"] }),
      ev(15, "change_status_changed", { change_id: "chg_c", from: "READY", to: "COMMITTED", reasons: [] }),
      ev(16, "job_status_changed", { job_id: "job_2", from: "RUNNING", to: "FAILED" }),
    ];
    const items = attention([], [job("job_u", "UNKNOWN")], events, null);
    expect(items.map((i) => i.kind)).toEqual(["unknown", "failed", "refused", "failed", "refused", "refused"]);
    expect(items.map((i) => i.seq)).toEqual([undefined, 16, 14, 13, 11, 10]);
    const failed = items.find((i) => i.seq === 13)!;
    expect(failed.detail).toContain("agent-7 kept for cleanup");
    expect(failed.refs).toContain("agent-7");
    expect(items.filter(isOpen)).toHaveLength(1);
    expect(counts(items)).toEqual({ action: 0, unknown: 1, waiting: 0, refused: 3, failed: 2 });
  });
});

suite("validAgentId", () => {
  it("mirrors the kernel's attach rule", () => {
    expect(validAgentId("6f1c2a9e-1111-4c3b-9d2e-000000000042")).toBe(true);
    expect(validAgentId("agent:creao_1.x")).toBe(true);
    expect(validAgentId("")).toBe(false);
    expect(validAgentId("bad id")).toBe(false);
    expect(validAgentId("../x")).toBe(false);
    expect(validAgentId("a".repeat(129))).toBe(false);
  });
});
