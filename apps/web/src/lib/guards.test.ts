// Henry's Claude: pins console decisions that mutation testing found
// unobserved (each `it` names the line it guards). What the console claims
// is what judges see, so each case asserts the claim, not the markup.
import { describe as suite, expect, it } from "vitest";
import type { CapabilityGap, CapabilityRunRecord, Installation, Job, OrgChangeRecord, OrgState } from "./contracts";
import { attention } from "./attention";
import { failureKind, holders, jobsForInstallation, summarizeRun } from "./capabilities";
import { executedBy, ownedBy } from "./graph";
import { intakeAttention, type GapIntakeStatus } from "./gapIntake";

const change = (operations: Record<string, unknown>) =>
  ({ change_id: "chg_1", status: "PROVISIONING", operations }) as unknown as OrgChangeRecord;
const op = (state: string, extra: Record<string, unknown> = {}) => ({
  provider: "creao",
  operation_id: "op-1",
  state,
  updated_at: "2026-10-22T00:00:00Z",
  ...extra,
});

suite("attention: a pending provider operation (attention.ts L98)", () => {
  it("is shown as waiting only when the provider reported a reason", () => {
    const items = attention([change({ monitor: op("pending", { attention: "rate limited; retrying" }) })], [], [], null);
    expect(items).toHaveLength(1);
    expect(items[0]!.kind).toBe("waiting");
    expect(items[0]!.title).toBe("Provider operation retrying");
    expect(items[0]!.detail).toContain("rate limited; retrying");
    expect(attention([change({ monitor: op("pending") })], [], [], null)).toEqual([]);
    // A reason on another state does not make it a retrying operation.
    expect(attention([change({ monitor: op("ready", { attention: "x" }) })], [], [], null)).toEqual([]);
  });
});

const run = (purpose: unknown) =>
  ({ job_id: "job_1", phase: "collected", purpose }) as unknown as CapabilityRunRecord;

suite("capabilities", () => {
  it("labels each run purpose exactly (L98)", () => {
    expect(summarizeRun(run({ kind: "invocation" })).purpose).toBe("invocation");
    expect(summarizeRun(run({ kind: "local_acceptance", suite_id: "s" })).purpose).toBe("local acceptance · s");
    expect(summarizeRun(run({ kind: "adoption_acceptance", suite_id: "s" })).purpose).toBe("adoption acceptance · s");
    expect(summarizeRun(run({ kind: "something_new" })).purpose).toBe("run");
    expect(summarizeRun(run(undefined)).purpose).toBe("run");
  });

  it("names an object failure by its variant, and an empty one as 'error' (L79)", () => {
    expect(failureKind({ Timeout: { secs: 3 } })).toBe("Timeout");
    expect(failureKind({})).toBe("error");
  });

  const inst = { installation_id: "inst_1", capability: { capability_id: "reader", version: "1.0.0" } } as unknown as Installation;
  const j = (id: string, created_at: string, version = "1.0.0") =>
    ({ job_id: id, created_at, capability: { capability_id: "reader", version } }) as unknown as Job;

  it("lists an installation's jobs newest first, and only its exact version (L153)", () => {
    const jobs = [j("old", "2026-10-22T01:00:00Z"), j("new", "2026-10-22T03:00:00Z"), j("mid", "2026-10-22T02:00:00Z"), j("other", "2026-10-22T04:00:00Z", "2.0.0")];
    expect(jobsForInstallation(jobs, inst).map((x) => x.job_id)).toEqual(["new", "mid", "old"]);
  });

  it("names only roles holding this exact capability version (L197)", () => {
    const state = {
      roles: {
        a: { name: "Holder", capabilities: [{ capability_id: "reader", version: "1.0.0" }] },
        b: { name: "Other version", capabilities: [{ capability_id: "reader", version: "2.0.0" }] },
        c: { name: "None", capabilities: [] },
      },
    } as unknown as OrgState;
    expect(holders(state, inst)).toEqual(["Holder"]);
  });
});

suite("graph: duties on a role card (L95, L101)", () => {
  const state = {
    responsibilities: {
      own: { responsibility_id: "own", status: "OPEN", owner: { kind: "role", id: "r1" }, executor: null },
      closed: { responsibility_id: "closed", status: "COMPLETED", owner: { kind: "role", id: "r1" }, executor: { kind: "role", id: "r1" } },
      other: { responsibility_id: "other", status: "OPEN", owner: { kind: "role", id: "r2" }, executor: { kind: "role", id: "r1" } },
      contracted: { responsibility_id: "contracted", status: "OPEN", owner: { kind: "unassigned", id: "r1" }, executor: { kind: "contractor", id: "r1" } },
    },
  } as unknown as OrgState;
  it("shows only open duties this role owns, as a role", () => {
    expect(ownedBy(state, "r1").map((r) => r.responsibility_id)).toEqual(["own"]);
  });
  it("shows only open duties this role executes, as a role", () => {
    expect(executedBy(state, "r1").map((r) => r.responsibility_id)).toEqual(["other"]);
  });
});

suite("gap intake attention (gapIntake.ts L107, L112)", () => {
  const gap = (status: unknown) =>
    ({ gap_id: "gap_1", capability_id: "venue-reader", role_id: "r1", opened_in_revision: 1, status }) as unknown as CapabilityGap;
  const st = (g: CapabilityGap) => ({ capability_gaps: { gap_1: g }, roles: { r1: { role_id: "r1", name: "Lead" } } }) as unknown as OrgState;
  const s = (state: string, code: string, acquisition_id: string | null) =>
    ({ gap_id: "gap_1", state, code, acquisition_id }) as unknown as GapIntakeStatus;

  it("cites the gap's own acquisition while acquiring, else the driver's", () => {
    const acquiring = intakeAttention(st(gap({ state: "acquiring", acquisition_id: "acq_gap" })), { gap_1: s("attention", "quota", "acq_driver") });
    expect(acquiring[0]!.refs).toEqual(["gap_1", "acq_gap"]);
    const open = intakeAttention(st(gap({ state: "open" })), { gap_1: s("attention", "quota", "acq_driver") });
    expect(open[0]!.refs).toEqual(["gap_1", "acq_driver"]);
  });

  it("says 'waiting' for a waiting code and 'needs an operator' otherwise", () => {
    const waiting = intakeAttention(st(gap({ state: "open" })), { gap_1: s("waiting", "provisioning", null) });
    expect(waiting[0]!.title).toBe("Capability request venue-reader is waiting");
    const action = intakeAttention(st(gap({ state: "open" })), { gap_1: s("attention", "quota", null) });
    expect(action[0]!.title).toBe("Capability request venue-reader needs an operator");
  });
});
