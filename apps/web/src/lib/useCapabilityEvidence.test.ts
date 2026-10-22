import { describe, expect, it } from "vitest";
import type { Job } from "./contracts";
import { fetchKey, NOT_MANAGED } from "./useCapabilityEvidence";

const job = (extra: Partial<Job> = {}) =>
  ({ job_id: "j", status: "RUNNING", raw_status: "RUNNING", provider_refs: { run_id: "r" }, updated_at: "1", ...extra }) as Job;

describe("fetchKey", () => {
  it("keeps re-reading an active run even when the job's coarse state is unchanged", () => {
    const running = { value: { job_id: "j", phase: "running" } };
    expect(fetchKey(job(), running, 1)).not.toBe(fetchKey(job(), running, 2));
  });

  it("re-reads after a retryable read failure and after a failed fetch", () => {
    const retryable = { value: { job_id: "j", phase: "collected", retryable_read: true } };
    expect(fetchKey(job(), retryable, 1)).not.toBe(fetchKey(job(), retryable, 2));
    const failed = { error: "network error" };
    expect(fetchKey(job(), failed, 1)).not.toBe(fetchKey(job(), failed, 2));
  });

  it("notices checkpoint changes through updated_at", () => {
    const done = { value: { job_id: "j", phase: "collected" } };
    expect(fetchKey(job({ updated_at: "1" }), done, 1)).not.toBe(fetchKey(job({ updated_at: "2" }), done, 1));
  });

  it("stops polling final records and non-managed jobs", () => {
    const done = { value: { job_id: "j", phase: "collected", retryable_read: false } };
    expect(fetchKey(job(), done, 1)).toBe(fetchKey(job(), done, 2));
    expect(fetchKey(job(), { error: NOT_MANAGED }, 1)).toBe(fetchKey(job(), { error: NOT_MANAGED }, 2));
  });
});
