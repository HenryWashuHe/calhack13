// Adopt-first in the console: open-gap row detail (taking precedence over
// gap intake while adoption owns the gap), Attention items, intake item
// suppression and the polling hook's stale-response rules. jsdom, stub API.
import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JSDOM } from "jsdom";
import { AttentionPanel } from "../components/AttentionPanel";
import type { Api } from "./api";
import type { CapabilityGap, OrgState } from "./contracts";
import { gapsForRole } from "./gaps";
import { GapIntakeContext, type GapIntakeStatus } from "./gapIntake";
import {
  ADOPTION_POLL_MS,
  adoptionAttention,
  adoptionDetail,
  GapAdoptionContext,
  openGaps,
  useGapAdoption,
  type GapAdoptionStatus,
} from "./gapAdoption";

const cap = { capability_id: "shared-venue-reader", version: "1.0.0" };
const gap = (id: string, rev: number, status: CapabilityGap["status"]): CapabilityGap =>
  ({
    gap_id: id, mission_id: "m", requirement_id: "r", requirement_digest: "d", triggering_evidence: ["o"],
    role_id: "role_b", responsibility_id: "rsp", capability_id: "capacity-checker", purpose: "Check capacity",
    observed_sources: ["https://portal.example/b"], status, opened_in_revision: rev,
  }) as CapabilityGap;
const gaps = {
  g_open: gap("g_open", 1, { state: "open" }),
  g_done: gap("g_done", 0, { state: "installed", acquisition_id: "adopt_1", capability: cap, installation_id: "i1" }),
};
const state = { roles: { role_b: { role_id: "role_b", name: "Verifier" } }, capability_gaps: gaps } as unknown as OrgState;
const status = (extra: Partial<GapAdoptionStatus>): GapAdoptionStatus => ({
  gap_id: "g_open", enabled: true, state: "active", code: null, adoption_id: null,
  synthesis_fallback: false, selected: null, installation_id: null, updated_at: null, ...extra,
});
const intakeReady: GapIntakeStatus = {
  gap_id: "g_open", enabled: true, state: "ready", code: null, acquisition_id: null, updated_at: null,
};

describe("gap adoption helpers", () => {
  it("polls only open gaps", () => {
    expect(openGaps(state).map((g) => g.gap_id)).toEqual(["g_open"]);
  });

  it("describes each stage and claims nothing without a record", () => {
    const g = gaps.g_open;
    expect(adoptionDetail(g, undefined)).toBeNull();
    expect(adoptionDetail(g, status({ state: null }))).toBeNull();
    expect(adoptionDetail(g, status({}))).toMatch(/preparing one adoption before any synthesis/);
    expect(adoptionDetail(g, status({ code: "conflict" }))).toMatch(/waiting for another adoption/);
    expect(adoptionDetail(g, status({ adoption_id: "adopt_9" }))).toMatch(/adoption adopt_9: searching/);
    expect(adoptionDetail(g, status({ state: "attention", code: "registry", adoption_id: "adopt_9" }))).toMatch(
      /registry search failed.*nothing falls back to synthesis \(adoption adopt_9\)/,
    );
    // Released hands the gap to synthesis; gap intake describes the rest.
    expect(adoptionDetail(g, status({ state: "released", code: "no_candidate" }))).toBeNull();
    // Non-open gaps keep the kernel's label.
    expect(adoptionDetail(gaps.g_done, status({ gap_id: "g_done", state: "installed" }))).toBeNull();
  });

  it("takes precedence over gap intake while adoption owns the gap", () => {
    const owned = gapsForRole(state, "role_b", { g_open: intakeReady }, {}, { g_open: status({ adoption_id: "a" }) });
    expect(owned[1]?.detail).toMatch(/adopt first/);
    expect(owned[1]?.detail).not.toMatch(/queued for automatic acquisition/);
    const released = gapsForRole(state, "role_b", { g_open: intakeReady }, {}, { g_open: status({ state: "released" }) });
    expect(released[1]?.detail).toMatch(/queued for automatic acquisition/);
  });

  it("raises attention for stopped adoption with the right kind and refs", () => {
    const items = adoptionAttention(state, { g_open: status({ state: "attention", code: "checks", adoption_id: "adopt_9" }) });
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ key: "gap-adoption:g_open:checks", kind: "unknown", refs: ["g_open", "adopt_9"] });
    expect(items[0]!.detail).toMatch(/nothing is resubmitted · requested by Verifier/);
    expect(adoptionAttention(state, { g_open: status({}) })).toEqual([]);
  });
});

let root: Root | undefined;
let dom: InstanceType<typeof JSDOM> | undefined;
afterEach(async () => {
  if (root) await act(async () => root?.unmount());
  root = undefined;
  dom?.window.close();
  dom = undefined;
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

function setup() {
  dom = new JSDOM("<div id='root'></div>");
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
  const container = dom.window.document.getElementById("root")!;
  root = createRoot(container);
  return container;
}

it("drops an older poll that answers after a newer one, and foreign gaps", async () => {
  const container = setup();
  const calls: { path: string; resolve: (s: GapAdoptionStatus) => void }[] = [];
  const api = { get: (path: string) => new Promise<GapAdoptionStatus>((resolve) => calls.push({ path, resolve })) } as unknown as Api;
  function Probe() {
    return <pre>{JSON.stringify(useGapAdoption(api, state))}</pre>;
  }
  await act(async () => root!.render(<Probe />));
  // Henry's Claude: the settled (installed) gap is read once for its outcome;
  // only the open gap is polled. The stale-response rules below are unchanged.
  const open = () => calls.filter((c) => c.path === "/capability-gaps/g_open/adoption");
  const settled = () => calls.filter((c) => c.path === "/capability-gaps/g_done/adoption");
  expect(open()).toHaveLength(1);
  expect(settled()).toHaveLength(1);
  await act(async () => { vi.advanceTimersByTime(ADOPTION_POLL_MS); });
  expect(open()).toHaveLength(2);
  expect(settled()).toHaveLength(1);
  await act(async () => open()[1]!.resolve(status({ adoption_id: "adopt_new" })));
  await act(async () => open()[0]!.resolve(status({ state: "attention", code: "registry" })));
  expect(JSON.parse(container.textContent!).g_open.adoption_id).toBe("adopt_new");
  await act(async () => { vi.advanceTimersByTime(ADOPTION_POLL_MS); });
  await act(async () => open()[2]!.resolve(status({ gap_id: "other", state: "attention", code: "checks" })));
  // A settled gap without an adoption record isn't kept.
  await act(async () => settled()[0]!.resolve(status({ gap_id: "g_done", state: null })));
  expect(Object.keys(JSON.parse(container.textContent!))).toEqual(["g_open"]);
});

it("an adopted gap's intake item is suppressed; its adoption item shows instead", async () => {
  const container = setup();
  const api = { get: () => new Promise(() => undefined) } as unknown as Api;
  // The driver is deferred, so its status may still say provisioning.
  const intake = { g_open: { ...intakeReady, state: "waiting", code: "provisioning" } as GapIntakeStatus };
  const render = (adoption: Record<string, GapAdoptionStatus>) =>
    root!.render(
      <GapAdoptionContext.Provider value={adoption}>
        <GapIntakeContext.Provider value={intake}>
          <AttentionPanel api={api} changes={[]} jobs={[]} events={[]} state={state} />
        </GapIntakeContext.Provider>
      </GapAdoptionContext.Provider>,
    );
  await act(async () => render({}));
  const intakeText = container.textContent;
  expect(intakeText).not.toBe("");
  await act(async () => render({ g_open: status({ state: "attention", code: "candidate", adoption_id: "adopt_9" }) }));
  expect(container.textContent).toContain("Adopting capacity-checker stopped");
  expect(container.textContent).toContain("adopt_9");
  expect(container.textContent).not.toContain(intakeText!.slice(0, 40));
});
