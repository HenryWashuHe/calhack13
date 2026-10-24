// Independent PR85 review: real hook and AttentionPanel lifecycle with stub HTTP.
import { afterEach, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JSDOM } from "jsdom";
import { AttentionPanel } from "../components/AttentionPanel";
import type { Api } from "./api";
import type { CapabilityGap, OrgState } from "./contracts";
import {
  ADOPTION_POLL_MS, GapAdoptionContext, useGapAdoption, type GapAdoptionStatus,
} from "./gapAdoption";

const cap = { capability_id: "shared-reader", version: "1.0.0" };
const gap = {
  gap_id: "gap-one", role_id: "reader", capability_id: "requested-reader",
  opened_in_revision: 1, observed_sources: [], status: { state: "open" },
} as unknown as CapabilityGap;
const state = {
  roles: { reader: { role_id: "reader", name: "Reader" } },
  capability_gaps: { "gap-one": gap },
} as unknown as OrgState;
const response = (extra: Partial<GapAdoptionStatus> = {}): GapAdoptionStatus => ({
  gap_id: "gap-one", enabled: true, state: "attention", code: "checks",
  adoption_id: "adoption-b", synthesis_fallback: false, selected: cap,
  installation_id: "inst-b", updated_at: null, ...extra,
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

it("drops previous-org and terminal-gap responses without retaining attention or polling", async () => {
  dom = new JSDOM("<div id='root'></div>");
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
  type Call = { path: string; resolve: (s: GapAdoptionStatus) => void };
  const callsA: Call[] = [];
  const callsB: Call[] = [];
  const api = (calls: Call[]) => ({
    get: (path: string) => new Promise<GapAdoptionStatus>((resolve) => calls.push({ path, resolve })),
  }) as unknown as Api;
  const a = api(callsA);
  const b = api(callsB);
  function Probe({ endpoint, snapshot }: { endpoint: Api; snapshot: OrgState }) {
    const statuses = useGapAdoption(endpoint, snapshot);
    return <GapAdoptionContext.Provider value={statuses}>
      <AttentionPanel api={endpoint} changes={[]} jobs={[]} events={[]} state={snapshot} />
    </GapAdoptionContext.Provider>;
  }
  const container = dom.window.document.getElementById("root")!;
  root = createRoot(container);
  await act(async () => root!.render(<Probe endpoint={a} snapshot={state} />));
  expect(callsA.map((c) => c.path)).toEqual(["/capability-gaps/gap-one/adoption"]);
  await act(async () => root!.render(<Probe endpoint={b} snapshot={state} />));
  await act(async () => callsA[0]!.resolve(response({ adoption_id: "org-a-adoption" })));
  expect(container.textContent).toBe("");
  await act(async () => callsB[0]!.resolve(response({ gap_id: "foreign-gap" })));
  expect(container.textContent).toBe("");
  await act(async () => { vi.advanceTimersByTime(ADOPTION_POLL_MS); });
  expect(callsA).toHaveLength(1);
  await act(async () => callsB[1]!.resolve(response()));
  expect(container.textContent).toContain("adoption-b");
  expect(container.textContent).toContain("provider outcome is unknown");
  await act(async () => { vi.advanceTimersByTime(ADOPTION_POLL_MS); });
  const installed = {
    ...state, capability_gaps: { "gap-one": {
      ...gap, status: { state: "installed", acquisition_id: "adoption-b", capability: cap, installation_id: "inst-b" },
    } },
  } as OrgState;
  await act(async () => root!.render(<Probe endpoint={b} snapshot={installed} />));
  await act(async () => callsB[2]!.resolve(response()));
  await act(async () => { vi.advanceTimersByTime(ADOPTION_POLL_MS * 2); });
  expect(callsB).toHaveLength(3);
  expect(container.textContent).toBe("");
});
