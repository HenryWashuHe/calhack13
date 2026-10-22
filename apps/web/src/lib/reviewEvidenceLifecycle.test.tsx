// PR45 dbbc775 review reproduction; no project dependency changes are required.
// From the repository root, install the isolated DOM dependency and run:
//   npm install --prefix /tmp/genesis-pr45-dom-review --no-package-lock --no-audit --no-fund jsdom@25.0.1
//   cd apps/web
//   npm test -- --run src/lib/reviewEvidenceLifecycle.test.tsx
// At dbbc775, the pre-fix negative control passes and the PR45 hook fails:
// expected "collected", received "running". This uses actual React DOM effects
// in jsdom, not browser interaction or a live provider.
import { afterEach, expect, it, vi } from "vitest";
import { act, StrictMode, useEffect, useMemo, useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
// @ts-expect-error Temporary review-only DOM dependency, no project package changes.
import { JSDOM } from "/tmp/genesis-pr45-dom-review/node_modules/jsdom/lib/api.js";
import { ApiError, type Api } from "./api";
import type { Job, CapabilityManifest, OrgState } from "./contracts";
import { capabilityKey, type CapabilityRunRecord } from "./capabilities";
import { useCapabilityEvidence } from "./useCapabilityEvidence";

let root: Root | undefined;
let dom: InstanceType<typeof JSDOM> | undefined;
afterEach(async () => {
  if (root) await act(async () => root?.unmount());
  root = undefined;
  dom?.window.close();
  dom = undefined;
  vi.unstubAllGlobals();
});

it.each([["pre-fix hook", useBaselineEvidence], ["PR45 hook", useCapabilityEvidence]] as const)("%s drops the disposed effect response after a newer checkpoint", async (_name, useEvidence) => {
  dom = new JSDOM("<div id='root'></div>");
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const requests: { resolve: (record: CapabilityRunRecord) => void }[] = [];
  const api = { get: () => new Promise<CapabilityRunRecord>((resolve) => requests.push({ resolve })) } as unknown as Api;
  const job = {
    job_id: "j1", status: "RUNNING", raw_status: "APIFY_SUBMITTED", provider_refs: {},
    created_at: "2026-10-22T00:00:00Z", updated_at: "2026-10-22T00:00:01Z",
    capability: { capability_id: "venue", version: "1" },
  } as unknown as Job;
  function Probe({ jobs }: { jobs: Job[] }) {
    const evidence = useEvidence(api, null, jobs);
    return <pre>{JSON.stringify(evidence.runs)}</pre>;
  }
  const container = dom.window.document.getElementById("root")!;
  root = createRoot(container);
  // Switching to the actual Capabilities tab mounts its hook with jobs already loaded.
  await act(async () => root!.render(<StrictMode><Probe jobs={[job]} /></StrictMode>));
  expect(requests).toHaveLength(2);
  await act(async () => root!.render(<StrictMode><Probe jobs={[{
    ...job, status: "VERIFYING", raw_status: "APIFY_COLLECTED_UNVERIFIED",
    updated_at: "2026-10-22T00:00:02Z",
  } as Job]} /></StrictMode>));
  expect(requests).toHaveLength(3);
  const collected: CapabilityRunRecord = { job_id: "j1", phase: "collected", purpose: { kind: "invocation" } };
  await act(async () => requests[2]!.resolve(collected));
  expect(JSON.parse(container.textContent!).j1.value.phase).toBe("collected");
  await act(async () => requests[0]!.resolve({ ...collected, phase: "running" }));
  // The first effect setup was disposed before either the newer job or response.
  expect(JSON.parse(container.textContent!).j1.value.phase).toBe("collected");
});

// Pre-fix hook from ae35622:apps/web/src/lib/useCapabilityEvidence.ts.
// Its behavior is unchanged; imports are shared above, exports removed, and
// the function renamed to useBaselineEvidence for this negative control.
// Fetches the evidence behind installed capabilities: the canonical manifest
// (pinned build, publisher verification) and each pinned run's backend record.
// Runs are refetched when their job changes; stale responses are ignored.

/** Error value for jobs without a backend capability run record. */
const NOT_MANAGED = "not_managed";

type Loaded<T> = { value: T } | { error: string };

interface CapabilityEvidence {
  manifests: Record<string, Loaded<CapabilityManifest>>;
  runs: Record<string, Loaded<CapabilityRunRecord>>;
}

const jobSignature = (j: Job) => `${j.status}|${j.raw_status ?? ""}|${JSON.stringify(j.provider_refs ?? {})}`;

function useBaselineEvidence(api: Api, state: OrgState | null, jobs: Job[]): CapabilityEvidence {
  const [manifests, setManifests] = useState<Record<string, Loaded<CapabilityManifest>>>({});
  const [runs, setRuns] = useState<Record<string, Loaded<CapabilityRunRecord>>>({});
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
      const sig = jobSignature(j);
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
  }, [api, jobs]);

  return { manifests, runs };
}
