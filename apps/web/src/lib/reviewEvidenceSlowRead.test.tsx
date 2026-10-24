// Henry's Claude, pressure test of #21 `abdaead`: an active run whose record
// read takes longer than REFRESH_MS. Each tick changes the fetch key, so the
// response for the previous key is dropped. Real React DOM effects in jsdom,
// fake timers; no live provider.
import { afterEach, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JSDOM } from "jsdom";
import type { Api } from "./api";
import type { Job } from "./contracts";
import type { CapabilityRunRecord } from "./capabilities";
import { REFRESH_MS, useCapabilityEvidence } from "./useCapabilityEvidence";

let root: Root | undefined;
let dom: InstanceType<typeof JSDOM> | undefined;
afterEach(async () => {
  if (root) await act(async () => root?.unmount());
  root = undefined;
  dom?.window.close();
  dom = undefined;
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it("shows an active run's record even when each read outlasts one refresh tick", async () => {
  vi.useFakeTimers();
  dom = new JSDOM("<div id='root'></div>");
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  // jsdom's window timers are not faked; route the hook's interval to the faked global ones.
  dom.window.setInterval = setInterval as never;
  dom.window.clearInterval = clearInterval as never;
  const latency = REFRESH_MS + 1000;
  let calls = 0;
  const record: CapabilityRunRecord = { job_id: "j1", phase: "running", purpose: { kind: "invocation" } };
  const api = {
    get: () => {
      calls += 1;
      return new Promise<CapabilityRunRecord>((resolve) => setTimeout(() => resolve(record), latency));
    },
  } as unknown as Api;
  const job = {
    job_id: "j1", status: "RUNNING", raw_status: "APIFY_SUBMITTED", provider_refs: {},
    created_at: "2026-10-22T00:00:00Z", updated_at: "2026-10-22T00:00:01Z",
    capability: { capability_id: "venue", version: "1" },
  } as unknown as Job;
  function Probe() {
    const evidence = useCapabilityEvidence(api, null, [job]);
    return <pre>{JSON.stringify(evidence.runs)}</pre>;
  }
  const container = dom.window.document.getElementById("root")!;
  root = createRoot(container);
  await act(async () => root!.render(<Probe />));
  for (let i = 0; i < 12; i++) {
    await act(async () => {
      await vi.advanceTimersByTimeAsync(REFRESH_MS);
    });
  }
  const runs = JSON.parse(container.textContent!);
  expect(calls).toBeGreaterThan(1);
  expect(runs.j1?.value?.phase, `after ${calls} reads over ${12 * REFRESH_MS} ms`).toBe("running");
});
