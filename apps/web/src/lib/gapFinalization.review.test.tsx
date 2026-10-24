// PR80 independent review: server-shaped eligibility payload and real hook /
// AttentionPanel lifecycle. These checks use only stub HTTP responses.
import { afterEach, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JSDOM } from "jsdom";
import { AttentionPanel } from "../components/AttentionPanel";
import type { Api } from "./api";
import type { CapabilityGap, OrgState } from "./contracts";
import { gapsForRole } from "./gaps";
import {
  FINALIZATION_POLL_MS, GapFinalizationContext, useGapFinalization,
  type GapFinalizationStatus,
} from "./gapFinalization";

const cap = { capability_id: "venue-reader", version: "1.0.0" };
const gap = {
  gap_id: "gap-one", role_id: "reader", capability_id: cap.capability_id,
  opened_in_revision: 1, observed_sources: [],
  status: { state: "acquired", acquisition_id: "acq-one", capability: cap, publication_ref: null },
} as unknown as CapabilityGap;
const state = {
  roles: { reader: { role_id: "reader", name: "Reader" } },
  capability_gaps: { "gap-one": gap },
} as unknown as OrgState;
const response = (extra: Partial<GapFinalizationStatus> = {}): GapFinalizationStatus => ({
  gap_id: "gap-one", enabled: true, state: "active", code: null, capability: cap,
  publication_ref: null, installation_id: null, job_ids: [], updated_at: null, ...extra,
});

it("global finalizer enablement does not claim this gap has queued work", () => {
  // Backend78: any configured policy sets enabled=true; with no matching rule
  // freeze returns Idle forever and the status remains this exact null record.
  const missingRule = response({ state: null, capability: null });
  const row = gapsForRole(state, "reader", {}, { "gap-one": missingRule })[0]!;
  expect(row.label).toBe("VERIFIED");
  expect(row.detail ?? "").not.toMatch(/queued|publishing|installing|running/i);
});

let root: Root | undefined;
let dom: JSDOM | undefined;
afterEach(async () => {
  if (root) await act(async () => root?.unmount());
  root = undefined;
  dom?.window.close();
  dom = undefined;
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

it("drops foreign and prior-org responses, then removes terminal-gap attention and polling", async () => {
  dom = new JSDOM("<div id='root'></div>");
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
  type Call = { path: string; resolve: (s: GapFinalizationStatus) => void };
  const callsA: Call[] = [];
  const callsB: Call[] = [];
  const api = (calls: Call[]) => ({
    get: (path: string) => new Promise<GapFinalizationStatus>((resolve) => calls.push({ path, resolve })),
  }) as unknown as Api;
  const a = api(callsA);
  const b = api(callsB);
  function Probe({ endpoint, snapshot }: { endpoint: Api; snapshot: OrgState }) {
    const statuses = useGapFinalization(endpoint, snapshot);
    return <GapFinalizationContext.Provider value={statuses}>
      <AttentionPanel api={endpoint} changes={[]} jobs={[]} events={[]} state={snapshot} />
    </GapFinalizationContext.Provider>;
  }
  const container = dom.window.document.getElementById("root")!;
  root = createRoot(container);
  await act(async () => root!.render(<Probe endpoint={a} snapshot={state} />));
  expect(callsA.map((c) => c.path)).toEqual(["/capability-gaps/gap-one/finalization"]);
  await act(async () => root!.render(<Probe endpoint={b} snapshot={state} />));
  expect(callsB).toHaveLength(1);
  await act(async () => callsA[0]!.resolve(response({ state: "attention", code: "publication" })));
  expect(container.textContent).toBe("");
  await act(async () => callsB[0]!.resolve(response({ gap_id: "foreign-gap", state: "attention", code: "checks" })));
  expect(container.textContent).toBe("");
  await act(async () => { vi.advanceTimersByTime(FINALIZATION_POLL_MS); });
  expect(callsA).toHaveLength(1);
  expect(callsB).toHaveLength(2);
  await act(async () => callsB[1]!.resolve(response({ state: "attention", code: "checks", job_ids: ["job-b"] })));
  expect(container.textContent).toContain("provider outcome is unknown");
  expect(container.textContent).toContain("job-b");
  await act(async () => { vi.advanceTimersByTime(FINALIZATION_POLL_MS); });
  expect(callsB).toHaveLength(3);
  const installed = {
    ...state, capability_gaps: { "gap-one": {
      ...gap, status: { state: "installed", acquisition_id: "acq-one", capability: cap, installation_id: "inst-b" },
    } },
  } as OrgState;
  await act(async () => root!.render(<Probe endpoint={b} snapshot={installed} />));
  await act(async () => callsB[2]!.resolve(response({ state: "attention", code: "checks" })));
  await act(async () => { vi.advanceTimersByTime(FINALIZATION_POLL_MS * 2); });
  expect(callsB).toHaveLength(3);
  expect(container.textContent).toBe("");
});
