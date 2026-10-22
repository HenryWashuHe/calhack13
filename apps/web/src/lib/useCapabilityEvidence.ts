// Fetches the evidence behind installed capabilities: the canonical manifest
// (pinned build, publisher verification) and each pinned run's backend record.
// Runs are refetched when their job changes (including updated_at), polled
// while active or awaiting a read retry, and retried after transient fetch
// errors; stale responses are ignored.

import { useEffect, useMemo, useRef, useState } from "react";
import { ApiError, type Api } from "./api";
import type { CapabilityManifest, Job, OrgState } from "./contracts";
import { capabilityKey, CapabilityRunRecord } from "./capabilities";

/** Error value for jobs without a backend capability run record. */
export const NOT_MANAGED = "not_managed";

export type Loaded<T> = { value: T } | { error: string };

export interface CapabilityEvidence {
  manifests: Record<string, Loaded<CapabilityManifest>>;
  runs: Record<string, Loaded<CapabilityRunRecord>>;
}

/**
 * Job fields that change whenever the backend saves the job, including a
 * collection read failure that leaves status/raw_status/provider_refs alone
 * but updates the run record's failure/retryable_read and `updated_at`.
 */
export const jobSignature = (j: Job) =>
  `${j.status}|${j.raw_status ?? ""}|${JSON.stringify(j.provider_refs ?? {})}|${j.updated_at ?? ""}`;

/** Run phases still moving on the backend. */
export const ACTIVE_PHASES = new Set(["prepared", "submitting", "running", "collecting"]);

/** What the hook knows about one job's run record. */
export interface RunFetch {
  sig: string;
  /** When the last request for this signature finished (ms), if it has. */
  at: number | null;
  outcome: "pending" | "ok" | "error" | "not_managed";
  phase?: string;
  retryableRead?: boolean;
}

export const POLL_MS = 3000;
export const RETRY_MS = 5000;

/**
 * Jobs whose run record should be (re)fetched now: never requested; job
 * changed since the last request; still active or awaiting a read retry
 * (polled even when the coarse job state is unchanged); or the last fetch
 * failed transiently. A 404 (not a backend run) and settled phases are not
 * refetched until the job changes.
 */
export function runsToFetch(jobs: Job[], known: Map<string, RunFetch>, now: number): string[] {
  const out: string[] = [];
  for (const j of jobs) {
    if (!j.capability) continue;
    const k = known.get(j.job_id);
    const sig = jobSignature(j);
    if (!k || k.sig !== sig) {
      out.push(j.job_id);
      continue;
    }
    if (k.outcome === "pending" || k.at === null) continue;
    const age = now - k.at;
    if (k.outcome === "error" && age >= RETRY_MS) out.push(j.job_id);
    else if (k.outcome === "ok" && (ACTIVE_PHASES.has(k.phase ?? "") || k.retryableRead) && age >= POLL_MS) {
      out.push(j.job_id);
    }
  }
  return out;
}

/** Whether any known run still needs polling. */
export function needsPolling(known: Map<string, RunFetch>): boolean {
  for (const k of known.values()) {
    if (k.outcome === "error" || (k.outcome === "ok" && (ACTIVE_PHASES.has(k.phase ?? "") || k.retryableRead))) return true;
  }
  return false;
}

export function useCapabilityEvidence(api: Api, state: OrgState | null, jobs: Job[]): CapabilityEvidence {
  const [manifests, setManifests] = useState<Record<string, Loaded<CapabilityManifest>>>({});
  const [runs, setRuns] = useState<Record<string, Loaded<CapabilityRunRecord>>>({});
  const requested = useRef<{ manifests: Set<string>; runs: Map<string, RunFetch> }>({
    manifests: new Set(),
    runs: new Map(),
  });
  // Advances while some run is active or failed, so those records are
  // re-read even when no job event arrives.
  const [tick, setTick] = useState(0);
  const [polling, setPolling] = useState(false);
  useEffect(() => {
    if (!polling) return;
    const t = setInterval(() => setTick((n) => n + 1), Math.min(POLL_MS, RETRY_MS));
    return () => clearInterval(t);
  }, [polling]);

  // The organization whose responses may be applied. Responses for another
  // organization (after a switch) or after unmount are dropped.
  const current = useRef<Api | null>(api);
  useEffect(() => {
    current.current = api;
    requested.current = { manifests: new Set(), runs: new Map() };
    setManifests({});
    setRuns({});
    return () => {
      current.current = null;
    };
  }, [api]);

  const installed = useMemo(
    () => Object.values(state?.installations ?? {}).map((i) => i.capability),
    [state],
  );
  useEffect(() => {
    const live = () => current.current === api;
    for (const cap of installed) {
      const key = capabilityKey(cap);
      if (requested.current.manifests.has(key)) continue;
      requested.current.manifests.add(key);
      api
        .get<CapabilityManifest>(
          `/capabilities/${encodeURIComponent(cap.capability_id)}/versions/${encodeURIComponent(cap.version)}`,
        )
        .then((m) => live() && setManifests((p) => ({ ...p, [key]: { value: m } })))
        .catch((e: unknown) => {
          if (!live()) return;
          requested.current.manifests.delete(key); // retried on a later state change
          setManifests((p) => ({ ...p, [key]: { error: e instanceof Error ? e.message : String(e) } }));
        });
    }
  }, [api, installed]);

  useEffect(() => {
    const live = () => current.current === api;
    const known = requested.current.runs;
    const byId = new Map(jobs.map((j) => [j.job_id, j]));
    for (const id of runsToFetch(jobs, known, Date.now())) {
      const sig = jobSignature(byId.get(id)!);
      known.set(id, { ...(known.get(id) ?? {}), sig, at: null, outcome: "pending" });
      const settle = (patch: Partial<RunFetch>) => {
        // Apply only while this request's map is still the current one: an
        // organization switch (or StrictMode's re-run of the reset effect)
        // replaces requested.current.runs, and a disposed request must not
        // write visible state or drive polling from the obsolete map.
        if (!live() || requested.current.runs !== known) return false;
        const k = known.get(id);
        // Ignore a response for an older job state.
        if (!k || k.sig !== sig) return false;
        known.set(id, { ...k, ...patch, at: Date.now() });
        setPolling(needsPolling(requested.current.runs));
        return true;
      };
      api
        .get<CapabilityRunRecord>(`/capability-runs/${encodeURIComponent(id)}`)
        .then((r) => {
          if (settle({ outcome: "ok", phase: r.phase, retryableRead: Boolean(r.retryable_read) })) {
            setRuns((p) => ({ ...p, [id]: { value: r } }));
          }
        })
        .catch((e: unknown) => {
          // 404: not a backend-managed run (e.g. a development stub job).
          const notManaged = e instanceof ApiError && e.status === 404;
          if (settle({ outcome: notManaged ? "not_managed" : "error" })) {
            const error = notManaged ? NOT_MANAGED : e instanceof Error ? e.message : String(e);
            // Keep showing the last good record during a transient failure.
            setRuns((p) => (!notManaged && p[id] && "value" in p[id]! ? p : { ...p, [id]: { error } }));
          }
        });
    }
  }, [api, jobs, tick]);

  return { manifests, runs };
}
