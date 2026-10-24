// Parent #21 refresh regressions, adapted to the single scheduler shared with #45.
import { describe, expect, it } from "vitest";
import type { Job } from "./contracts";
import { jobSignature, needsPolling, POLL_MS, RETRY_MS, runsToFetch, type RunFetch } from "./useCapabilityEvidence";

const job = (extra: Partial<Job> = {}) =>
  ({ job_id: "j", status: "RUNNING", raw_status: "RUNNING", provider_refs: { run_id: "r" }, updated_at: "1",
    capability: { capability_id: "venue", version: "1" }, ...extra }) as Job;
const known = (j: Job, entry: Omit<RunFetch, "sig">) => new Map([[j.job_id, { sig: jobSignature(j), ...entry }]]);

describe("parent refresh behavior on the shared scheduler", () => {
  it("keeps re-reading an active run even when the job's coarse state is unchanged", () => {
    const j = job();
    const running = known(j, { outcome: "ok", phase: "running", at: 1000 });
    expect(runsToFetch([j], running, 1000 + POLL_MS - 1)).toEqual([]);
    expect(runsToFetch([j], running, 1000 + POLL_MS)).toEqual(["j"]);
  });

  it("re-reads after a retryable read failure and after a failed fetch", () => {
    const j = job();
    const retryable = known(j, { outcome: "ok", phase: "collected", retryableRead: true, at: 0 });
    expect(needsPolling(retryable)).toBe(true);
    expect(runsToFetch([j], retryable, POLL_MS)).toEqual(["j"]);
    const failed = known(j, { outcome: "error", at: 0 });
    expect(runsToFetch([j], failed, RETRY_MS - 1)).toEqual([]);
    expect(runsToFetch([j], failed, RETRY_MS)).toEqual(["j"]);
  });

  it("notices checkpoint changes through updated_at", () => {
    const j = job();
    const done = known(j, { outcome: "ok", phase: "collected", at: 0 });
    expect(runsToFetch([job({ updated_at: "2" })], done, 1)).toEqual(["j"]);
  });

  it("stops polling final records and non-managed jobs", () => {
    const j = job();
    for (const phase of ["collected", "failed", "rejected", "needs_reconciliation"]) {
      const done = known(j, { outcome: "ok", phase, retryableRead: false, at: 0 });
      expect(runsToFetch([j], done, RETRY_MS)).toEqual([]);
      expect(needsPolling(done)).toBe(false);
    }
    const unmanaged = known(j, { outcome: "not_managed", at: 0 });
    expect(runsToFetch([j], unmanaged, RETRY_MS)).toEqual([]);
    expect(needsPolling(unmanaged)).toBe(false);
  });
});
