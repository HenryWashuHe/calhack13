// PR #21 review regressions: installation-bound acceptance evidence and
// refreshing run records whose coarse job state does not change.
import { describe, expect, it } from "vitest";
import type { Installation, Job } from "./contracts";
import { partitionJobs, summarizeRun, type CapabilityRunRecord } from "./capabilities";
import { jobSignature, needsPolling, POLL_MS, RETRY_MS, runsToFetch, type RunFetch } from "./useCapabilityEvidence";

const cap = { capability_id: "venue-extractor", version: "1.0.0" };
const job = (id: string, at = "2026-10-22T00:00:00Z", extra: Partial<Job> = {}) =>
  ({ job_id: id, created_at: at, updated_at: at, status: "RUNNING", raw_status: null, provider_refs: {}, capability: cap, ...extra }) as unknown as Job;
const inst = (id: string) => ({ installation_id: id, capability: cap }) as unknown as Installation;
const acceptance = (job_id: string, installation_id: string, kind = "local_acceptance"): CapabilityRunRecord =>
  ({ job_id, phase: "collected", purpose: { kind, installation_id, suite_id: "suite-b", adoption_id: "ad-1" } }) as CapabilityRunRecord;

describe("acceptance evidence is bound to its installation", () => {
  it("two installations of one exact version do not share acceptance runs", () => {
    const jobs = [job("j1", "1"), job("j2", "2"), job("j3", "3"), job("j4", "4")];
    const records = {
      j1: acceptance("j1", "inst_old"),
      j2: acceptance("j2", "inst_new", "adoption_acceptance"),
      j3: { job_id: "j3", phase: "collected", purpose: { kind: "invocation" } } as CapabilityRunRecord,
      // j4: record not loaded yet
    };
    const old = partitionJobs(jobs, inst("inst_old"), records);
    const fresh = partitionJobs(jobs, inst("inst_new"), records);
    expect(old.bound.map((j) => j.job_id)).toEqual(["j1"]);
    expect(fresh.bound.map((j) => j.job_id)).toEqual(["j2"]);
    // Invocations and unloaded records are shown separately, never as acceptance.
    expect(old.unbound.map((j) => j.job_id)).toEqual(["j4", "j3"]);
    expect(fresh.unbound.map((j) => j.job_id)).toEqual(["j4", "j3"]);
  });

  it("labels adoption acceptance runs", () => {
    expect(summarizeRun(acceptance("j", "i", "adoption_acceptance")).purpose).toBe("adoption acceptance · suite-b");
  });
});

describe("run records refresh without a coarse job change", () => {
  const known = (entries: Record<string, RunFetch>) => new Map(Object.entries(entries));

  it("refetches after a read failure that only changes updated_at", () => {
    const before = job("j1", "2026-10-22T00:00:00Z", { updated_at: "2026-10-22T00:00:01Z" } as Partial<Job>);
    const after = { ...before, updated_at: "2026-10-22T00:00:09Z" } as Job;
    // Same status / raw_status / provider_refs: the old signature missed this.
    expect(after.status).toBe(before.status);
    const k = known({ j1: { sig: jobSignature(before), at: 1000, outcome: "ok", phase: "running" } });
    expect(runsToFetch([after], k, 1001)).toEqual(["j1"]);
  });

  it("polls an active run even when the job does not change", () => {
    const j = job("j1");
    const k = known({ j1: { sig: jobSignature(j), at: 1000, outcome: "ok", phase: "running" } });
    expect(runsToFetch([j], k, 1000 + POLL_MS - 1)).toEqual([]);
    expect(runsToFetch([j], k, 1000 + POLL_MS)).toEqual(["j1"]);
    expect(needsPolling(k)).toBe(true);
  });

  it("keeps polling a settled phase that awaits an operator read retry", () => {
    const j = job("j1");
    const k = known({ j1: { sig: jobSignature(j), at: 0, outcome: "ok", phase: "collecting", retryableRead: true } });
    expect(runsToFetch([j], k, POLL_MS)).toEqual(["j1"]);
    const record = { job_id: "j1", phase: "collecting", retryable_read: true } as CapabilityRunRecord;
    expect(summarizeRun(record).attention).toMatch(/retry the read/);
  });

  it("retries a transient fetch failure but not a 404 or a settled run", () => {
    const j = job("j1");
    const sig = jobSignature(j);
    expect(runsToFetch([j], known({ j1: { sig, at: 0, outcome: "error" } }), RETRY_MS)).toEqual(["j1"]);
    expect(runsToFetch([j], known({ j1: { sig, at: 0, outcome: "error" } }), RETRY_MS - 1)).toEqual([]);
    expect(runsToFetch([j], known({ j1: { sig, at: 0, outcome: "not_managed" } }), 1e9)).toEqual([]);
    const settled = known({ j1: { sig, at: 0, outcome: "ok", phase: "collected" } });
    expect(runsToFetch([j], settled, 1e9)).toEqual([]);
    expect(needsPolling(settled)).toBe(false);
  });

  it("does not issue a second request while one is in flight for the same job state", () => {
    const j = job("j1");
    expect(runsToFetch([j], known({ j1: { sig: jobSignature(j), at: null, outcome: "pending" } }), 1e9)).toEqual([]);
    expect(runsToFetch([j], new Map(), 0)).toEqual(["j1"]);
    expect(runsToFetch([{ ...j, capability: null } as unknown as Job], new Map(), 0)).toEqual([]);
  });
});
