// Fetches the evidence behind installed capabilities: the canonical manifest
// (pinned build, publisher verification) and each pinned run's backend record.
// Runs are refetched when their job changes; stale responses are ignored.

import { useEffect, useMemo, useRef, useState } from "react";
import { ApiError, type Api } from "./api";
import type { CapabilityManifest, Job, OrgState } from "./contracts";
import { capabilityKey, CapabilityRunRecord, FINAL_PHASES } from "./capabilities";

/** Error value for jobs without a backend capability run record. */
export const NOT_MANAGED = "not_managed";

export type Loaded<T> = { value: T } | { error: string };

export interface CapabilityEvidence {
  manifests: Record<string, Loaded<CapabilityManifest>>;
  runs: Record<string, Loaded<CapabilityRunRecord>>;
}

/** How often active run checkpoints and failed reads are re-read. */
export const REFRESH_MS = 5000;

/**
 * Key under which a job's run record was last requested; a new key means
 * fetch again. It changes with the job (including `updated_at`, which moves
 * on checkpoint changes the coarse status does not show) and, while the run
 * is still active or its last read failed, with every refresh tick, so a
 * retryable read failure or a new checkpoint is observed even when the job's
 * status stays the same. A final record or a non-managed job stops polling.
 */
export function fetchKey(job: Job, loaded: Loaded<CapabilityRunRecord> | undefined, tick: number): string {
  const base = `${job.status}|${job.raw_status ?? ""}|${JSON.stringify(job.provider_refs ?? {})}|${job.updated_at}`;
  const settled =
    loaded !== undefined &&
    ("value" in loaded ? FINAL_PHASES.has(loaded.value.phase) && !loaded.value.retryable_read : loaded.error === NOT_MANAGED);
  return settled ? base : `${base}|t${tick}`;
}

export function useCapabilityEvidence(api: Api, state: OrgState | null, jobs: Job[]): CapabilityEvidence {
  const [manifests, setManifests] = useState<Record<string, Loaded<CapabilityManifest>>>({});
  const [runs, setRuns] = useState<Record<string, Loaded<CapabilityRunRecord>>>({});
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => setTick((t) => t + 1), REFRESH_MS);
    return () => window.clearInterval(id);
  }, []);
  const requested = useRef<{ manifests: Set<string>; runs: Map<string, string> }>({
    manifests: new Set(),
    runs: new Map(),
  });

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
    for (const j of jobs) {
      if (!j.capability) continue;
      const sig = fetchKey(j, runs[j.job_id], tick);
      if (requested.current.runs.get(j.job_id) === sig) continue;
      requested.current.runs.set(j.job_id, sig);
      api
        .get<CapabilityRunRecord>(`/capability-runs/${encodeURIComponent(j.job_id)}`)
        .then((r) => {
          // Ignore a response for an older job state.
          if (live() && requested.current.runs.get(j.job_id) === sig) setRuns((p) => ({ ...p, [j.job_id]: { value: r } }));
        })
        .catch((e: unknown) => {
          if (live() && requested.current.runs.get(j.job_id) === sig) {
            // 404: not a backend-managed run (e.g. a development stub job).
            const error = e instanceof ApiError && e.status === 404 ? NOT_MANAGED : e instanceof Error ? e.message : String(e);
            setRuns((p) => ({ ...p, [j.job_id]: { error } }));
          }
        });
    }
  }, [api, jobs, runs, tick]);

  return { manifests, runs };
}
