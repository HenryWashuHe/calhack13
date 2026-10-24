// Capability evidence for the console: what was installed, which exact
// Actor build it pins, what verification the publisher attached, and what
// each pinned run actually returned. Pure helpers here; fetching is in
// useCapabilityEvidence.

import type { CapabilityManifest, Installation, Job, OrgState } from "./contracts";

/** Marker the registry uses for development (assertion-only) publications. */
export const DEV_ASSERTION = "genesis:development-assertion";

/**
 * Backend capability run record (`GET /capability-runs/{job_id}`). Not part of
 * the generated contracts; fields are read defensively.
 */
export interface CapabilityRunRecord {
  job_id: string;
  phase: string;
  purpose?:
    | { kind: "invocation" }
    | { kind: "local_acceptance"; installation_id: string; suite_id: string }
    | { kind: "adoption_acceptance"; adoption_id: string; installation_id: string; suite_id: string };
  request?: {
    case_id?: string;
    artifact?: { actor_id?: string; build_id?: string; build_number?: string; capability_id?: string; capability_version?: string };
    source?: { url?: string; sha256?: string };
  };
  receipt?: { provider?: string; provider_refs?: Record<string, string>; raw_status?: string } | null;
  evidence?: {
    run_id?: string;
    dataset_id?: string;
    status?: string;
    output?: unknown;
  } | null;
  failure?: unknown;
  retryable_read?: boolean;
}

export type Tone = "ok" | "warn" | "bad" | undefined;

export interface RunSummary {
  jobId: string;
  phase: string;
  phaseTone: Tone;
  purpose: string;
  caseId: string | null;
  runId: string | null;
  datasetId: string | null;
  providerStatus: string | null;
  output: string | null;
  failure: string | null;
  /** Operator attention needed, if any. */
  attention: string | null;
}

/** Short, non-sensitive description of an Actor output. */
export function describeOutput(output: unknown): string | null {
  if (output === undefined || output === null) return null;
  if (typeof output === "object" && !Array.isArray(output)) {
    const o = output as Record<string, unknown>;
    if (o.status === "supported" && Array.isArray(o.venues)) {
      const n = o.venues.length;
      return `${n} record${n === 1 ? "" : "s"} extracted`;
    }
    if (o.status === "unsupported") {
      return `unsupported: ${String(o.reason ?? "unspecified").replace(/_/g, " ")}`;
    }
    return `${Object.keys(o).length} fields`;
  }
  if (Array.isArray(output)) return `${output.length} items`;
  return "scalar output";
}

/** The variant name of a serialized provider error, never its text. */
export function failureKind(failure: unknown): string | null {
  if (failure === undefined || failure === null) return null;
  if (typeof failure === "string") return failure.replace(/[^A-Za-z0-9_ ]/g, "").slice(0, 40) || "error";
  if (typeof failure === "object") {
    const keys = Object.keys(failure as object);
    return keys.length > 0 ? keys[0]!.replace(/[^A-Za-z0-9_]/g, "").slice(0, 40) : "error";
  }
  return "error";
}

const PHASE_TONE: Record<string, Tone> = {
  collected: "ok",
  failed: "bad",
  rejected: "bad",
  needs_reconciliation: "bad",
};

export function summarizeRun(r: CapabilityRunRecord): RunSummary {
  const refs = r.receipt?.provider_refs ?? {};
  const purpose =
    r.purpose?.kind === "local_acceptance"
      ? `local acceptance · ${r.purpose.suite_id}`
      : r.purpose?.kind === "adoption_acceptance"
        ? `adoption acceptance · ${r.purpose.suite_id}`
        : r.purpose?.kind === "invocation"
          ? "invocation"
          : "run";
  let attention: string | null = null;
  if (r.phase === "needs_reconciliation") {
    attention = "Submission outcome unknown; reconcile with the provider before any new attempt.";
  } else if (r.retryable_read) {
    attention = "A read failed; an operator can retry the read (no resubmission).";
  }
  return {
    jobId: r.job_id,
    phase: r.phase,
    phaseTone: PHASE_TONE[r.phase],
    purpose,
    caseId: r.request?.case_id ?? null,
    runId: r.evidence?.run_id ?? refs.run_id ?? refs.runId ?? null,
    datasetId: r.evidence?.dataset_id ?? refs.dataset_id ?? refs.datasetId ?? null,
    providerStatus: r.evidence?.status ?? r.receipt?.raw_status ?? null,
    output: describeOutput(r.evidence?.output),
    failure: failureKind(r.failure),
    attention,
  };
}

export interface VerificationSummary {
  passed: number;
  total: number;
  allPassed: boolean;
  environment: string;
  oracle: string;
  limitations: string[];
  devAssertion: boolean;
}

export function summarizeVerification(m: CapabilityManifest): VerificationSummary {
  const cases = m.verification?.cases ?? [];
  const passed = cases.filter((c) => c.passed).length;
  return {
    passed,
    total: cases.length,
    allPassed: Boolean(m.verification?.passed) && passed === cases.length && cases.length > 0,
    environment: m.verification?.tested_environment ?? "",
    oracle: m.verification?.oracle_ref ?? "",
    limitations: [...(m.verification?.limitations ?? []), ...(m.known_limitations ?? [])],
    devAssertion: (m.provenance ?? []).includes(DEV_ASSERTION),
  };
}

export const capabilityKey = (c: { capability_id: string; version: string }) => `${c.capability_id}@${c.version}`;

/** Installation an acceptance run belongs to, from the backend run purpose. */
export function acceptanceInstallation(r: CapabilityRunRecord | undefined): string | null {
  const p = r?.purpose;
  return p && (p.kind === "local_acceptance" || p.kind === "adoption_acceptance") ? p.installation_id : null;
}

export interface InstallationRuns {
  /** Acceptance runs whose backend purpose names this installation. */
  acceptance: Job[];
  /**
   * Other runs of the same capability version: invocations and runs whose
   * record is not loaded or not backend-managed. Not evidence for this
   * installation's acceptance.
   */
  other: Job[];
}

/**
 * Split a capability version's runs for one installation card. Acceptance
 * evidence is bound by the run's installation ID, never by version alone, so
 * two installations of one version never show each other's acceptance runs.
 */
export function runsForInstallation(
  jobs: Job[],
  records: Record<string, CapabilityRunRecord | undefined>,
  inst: Installation,
): InstallationRuns {
  const acceptance: Job[] = [];
  const other: Job[] = [];
  for (const j of jobsForInstallation(jobs, inst)) {
    const owner = acceptanceInstallation(records[j.job_id]);
    if (owner === inst.installation_id) acceptance.push(j);
    else if (owner === null) other.push(j);
    // else: another installation's acceptance run
  }
  return { acceptance, other };
}

/** Run phases after which the backend record no longer changes by itself. */
export const FINAL_PHASES = new Set(["collected", "failed", "rejected"]);

/** Capability jobs for this exact capability version, newest first. */
export function jobsForInstallation(jobs: Job[], inst: Installation): Job[] {
  const key = capabilityKey(inst.capability);
  return jobs
    .filter((j) => j.capability && capabilityKey(j.capability) === key)
    .sort((a, b) => (a.created_at < b.created_at ? 1 : -1));
}

/** Roles that currently hold this exact capability version. */
export function holders(state: OrgState | null, inst: Installation): string[] {
  const key = capabilityKey(inst.capability);
  return Object.values(state?.roles ?? {})
    .filter((r) => (r.capabilities ?? []).some((c) => capabilityKey(c) === key))
    .map((r) => r.name);
}
