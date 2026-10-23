// Gap finalization in the console: acquired-gap row detail, Attention items
// and the polling hook's stale-response rules. jsdom effects, stub API.
import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JSDOM } from "jsdom";
import type { Api } from "./api";
import type { CapabilityGap, OrgState } from "./contracts";
import { gapsForRole } from "./gaps";
import {
  FINALIZATION_POLL_MS,
  finalizationAttention,
  finalizationDetail,
  useGapFinalization,
  verifiedGaps,
  type GapFinalizationStatus,
} from "./gapFinalization";

const cap = { capability_id: "venue-reader", version: "1.0.0" };
const gap = (id: string, rev: number, status: CapabilityGap["status"]): CapabilityGap =>
  ({
    gap_id: id, mission_id: "m", requirement_id: "r", requirement_digest: "d", triggering_evidence: ["o"],
    role_id: "role_a", responsibility_id: "rsp", capability_id: "venue-reader", purpose: "Read venues",
    observed_sources: ["https://portal.example/a"], status, opened_in_revision: rev,
  }) as CapabilityGap;
const gaps = {
  g_ver: gap("g_ver", 1, { state: "acquired", acquisition_id: "acq-1", capability: cap, publication_ref: null }),
  g_open: gap("g_open", 2, { state: "open" }),
  g_done: gap("g_done", 0, { state: "installed", acquisition_id: "acq-0", capability: cap, installation_id: "i0" }),
};
const state = { roles: { role_a: { role_id: "role_a", name: "Lead" } }, capability_gaps: gaps } as unknown as OrgState;
const status = (extra: Partial<GapFinalizationStatus>): GapFinalizationStatus => ({
  gap_id: "g_ver", enabled: true, state: "active", code: null, capability: cap,
  publication_ref: null, installation_id: null, job_ids: [], updated_at: null, ...extra,
});

describe("gap finalization helpers", () => {
  it("polls only verified gaps", () => {
    expect(verifiedGaps(state).map((g) => g.gap_id)).toEqual(["g_ver"]);
  });

  it("describes each stage without claiming installation", () => {
    const g = gaps.g_ver;
    expect(finalizationDetail(g, undefined)).toBeNull();
    expect(finalizationDetail(g, status({ enabled: false }))).toBeNull();
    expect(finalizationDetail(g, status({ state: null }))).toMatch(/queued for publication/);
    expect(finalizationDetail(g, status({}))).toMatch(/publishing this exact version/);
    expect(finalizationDetail(g, status({ publication_ref: "registry:venue-reader@1.0.0" }))).toMatch(/installing here/);
    expect(finalizationDetail(g, status({ publication_ref: "p", installation_id: "inst-1" }))).toMatch(/queuing local acceptance/);
    expect(finalizationDetail(g, status({ publication_ref: "p", installation_id: "inst-1", job_ids: ["a", "b", "c"] }))).toMatch(
      /3 local acceptance checks running/,
    );
    expect(finalizationDetail(g, status({ state: "attention", code: "publication" }))).toMatch(/registry does not hold exactly/);
    // Installed/refused: the kernel's own label stays.
    expect(finalizationDetail(g, status({ state: "installed" }))).toBeNull();
    expect(finalizationDetail(gaps.g_done, status({ gap_id: "g_done" }))).toBeNull();
    const row = gapsForRole(state, "role_a", {}, { g_ver: status({}) }).find((r) => r.gapId === "g_ver");
    expect(row?.label).toBe("VERIFIED");
    expect(row?.detail).toMatch(/publishing/);
  });

  it("raises attention for stopped finalization with the right kind and refs", () => {
    const items = finalizationAttention(state, {
      g_ver: status({ state: "attention", code: "checks", installation_id: "inst-1", job_ids: ["j1", "j2"] }),
    });
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ key: "gap-finalization:g_ver:checks", kind: "unknown", refs: ["g_ver", "inst-1", "j1", "j2"] });
    expect(items[0]!.detail).toMatch(/nothing is resubmitted · requested by Lead/);
    expect(finalizationAttention(state, { g_ver: status({}) })).toEqual([]);
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

it("drops an older poll that answers after a newer one", async () => {
  dom = new JSDOM("<div id='root'></div>");
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
  const calls: { path: string; resolve: (s: GapFinalizationStatus) => void }[] = [];
  const api = { get: (path: string) => new Promise<GapFinalizationStatus>((resolve) => calls.push({ path, resolve })) } as unknown as Api;
  function Probe() {
    return <pre>{JSON.stringify(useGapFinalization(api, state))}</pre>;
  }
  const container = dom.window.document.getElementById("root")!;
  root = createRoot(container);
  await act(async () => root!.render(<Probe />));
  expect(calls.map((c) => c.path)).toEqual(["/capability-gaps/g_ver/finalization"]);
  await act(async () => { vi.advanceTimersByTime(FINALIZATION_POLL_MS); });
  expect(calls).toHaveLength(2);
  await act(async () => calls[1]!.resolve(status({ publication_ref: "p" })));
  await act(async () => calls[0]!.resolve(status({ state: "attention", code: "publication" })));
  const shown = JSON.parse(container.textContent!).g_ver;
  expect(shown.state).toBe("active");
  expect(shown.publication_ref).toBe("p");
});
