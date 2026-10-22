import { describe, expect, it } from "vitest";
import type { CapabilityManifest, Installation, Job } from "./contracts";
import {
  DEV_ASSERTION,
  describeOutput,
  failureKind,
  jobsForInstallation,
  summarizeRun,
  summarizeVerification,
} from "./capabilities";

describe("summarizeRun", () => {
  it("reports an acceptance run's exact provider identities and output", () => {
    const s = summarizeRun({
      job_id: "job_1",
      phase: "collected",
      purpose: { kind: "local_acceptance", installation_id: "inst_1", suite_id: "org-b-suite" },
      request: { case_id: "case-a" },
      receipt: { provider: "apify", provider_refs: { run_id: "run-1" }, raw_status: "SUCCEEDED" },
      evidence: { run_id: "run-1", dataset_id: "ds-1", status: "succeeded", output: { status: "supported", venues: [{}, {}] } },
      failure: null,
      retryable_read: false,
    });
    expect(s).toMatchObject({
      purpose: "local acceptance · org-b-suite",
      phaseTone: "ok",
      caseId: "case-a",
      runId: "run-1",
      datasetId: "ds-1",
      providerStatus: "succeeded",
      output: "2 records extracted",
      failure: null,
      attention: null,
    });
  });

  it("flags unknown submissions and retryable reads without showing provider text", () => {
    const unknown = summarizeRun({
      job_id: "j",
      phase: "needs_reconciliation",
      failure: { Unknown: "token=secret echoed by provider" },
    });
    expect(unknown.phaseTone).toBe("bad");
    expect(unknown.failure).toBe("Unknown");
    expect(unknown.attention).toMatch(/reconcile/);
    expect(JSON.stringify(unknown)).not.toContain("secret");
    expect(summarizeRun({ job_id: "j", phase: "running", retryable_read: true }).attention).toMatch(/retry the read/);
  });
});

describe("output and failure descriptions", () => {
  it("describes unsupported outputs by reason", () => {
    expect(describeOutput({ status: "unsupported", reason: "unsupported_format" })).toBe("unsupported: unsupported format");
    expect(describeOutput(null)).toBeNull();
    expect(failureKind({ Http: { status: 500, body: "x" } })).toBe("Http");
  });
});

const manifest = (provenance: string[], passed: boolean[]): CapabilityManifest =>
  ({
    capability_id: "venue-extractor",
    version: "1.0.0",
    publisher: "org_a",
    source_digest: "sha256:abc",
    intent: "Extract venues",
    input_schema: {},
    output_schema: {},
    artifact: { actor_id: "actor-1", build_id: "build-1", build_number: "0.0.3" },
    acceptance_suite_ref: "suite",
    verification: {
      passed: passed.every(Boolean),
      cases: passed.map((p, i) => ({ name: `case-${i}`, passed: p })),
      tested_environment: "fixture portal",
      oracle_ref: "oracle-1",
      limitations: ["fixtures only"],
      verified_at: "2026-10-21T00:00:00Z",
    },
    provenance,
    state: "PUBLISHED",
  }) as unknown as CapabilityManifest;

describe("summarizeVerification", () => {
  it("counts passed cases and marks development assertions", () => {
    const v = summarizeVerification(manifest([DEV_ASSERTION], [true, true, false]));
    expect(v).toMatchObject({ passed: 2, total: 3, allPassed: false, devAssertion: true });
    expect(summarizeVerification(manifest([], [true, true, true])).allPassed).toBe(true);
  });
});

describe("jobsForInstallation", () => {
  it("selects jobs for the exact capability version, newest first", () => {
    const inst = { capability: { capability_id: "venue-extractor", version: "1.0.0" } } as Installation;
    const job = (id: string, version: string, at: string) =>
      ({ job_id: id, created_at: at, capability: { capability_id: "venue-extractor", version } }) as unknown as Job;
    const jobs = [job("a", "1.0.0", "1"), job("b", "2.0.0", "2"), job("c", "1.0.0", "3")];
    expect(jobsForInstallation(jobs, inst).map((j) => j.job_id)).toEqual(["c", "a"]);
  });
});
