// PR #45 regression (Codex review reproduction b61e21c on
// codex/review-evidence-fix45): under StrictMode, a request issued by a
// disposed effect must not overwrite a newer checkpoint. Real React DOM
// effects in jsdom; not a browser interaction or a live provider. The
// pre-fix negative control stays on that branch.
import { afterEach, expect, it, vi } from "vitest";
import { act, StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JSDOM } from "jsdom";
import type { Api } from "./api";
import type { Job } from "./contracts";
import type { CapabilityRunRecord } from "./capabilities";
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

it("drops the disposed effect response after a newer checkpoint", async () => {
  const useEvidence = useCapabilityEvidence;
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
