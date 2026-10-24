// Gap intake status in the console: role rows and Attention items derived from
// the server's gap driver status, and the polling hook's stale-response rules.
// jsdom React effects with a stub API; no server or provider.
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JSDOM } from "jsdom";
import type { Api } from "./api";
import type { CapabilityGap, OrgState } from "./contracts";
import { gapsForRole } from "./gaps";
import { intakeAttention, intakeDetail, liveGaps, useGapIntake, type GapIntakeStatus } from "./gapIntake";

const cap = { capability_id: "venue-reader", version: "1.0.0" };
const gap = (id: string, rev: number, status: CapabilityGap["status"]): CapabilityGap =>
  ({
    gap_id: id,
    mission_id: "m",
    requirement_id: "r",
    requirement_digest: "d",
    triggering_evidence: ["o"],
    role_id: "role_a",
    responsibility_id: "rsp",
    capability_id: "venue-reader",
    purpose: "Read venues",
    observed_sources: ["https://portal.example/a"],
    status,
    opened_in_revision: rev,
  }) as CapabilityGap;
const status = (id: string, extra: Partial<GapIntakeStatus>): GapIntakeStatus => ({
  gap_id: id,
  enabled: true,
  state: "ready",
  code: null,
  acquisition_id: null,
  updated_at: null,
  ...extra,
});
const gaps = {
  g_open: gap("g_open", 2, { state: "open" }),
  g_acq: gap("g_acq", 1, { state: "acquiring", acquisition_id: "acq-1" }),
  g_done: gap("g_done", 0, { state: "installed", acquisition_id: "acq-0", capability: cap, installation_id: "i" }),
};
const state = {
  roles: { role_a: { role_id: "role_a", name: "Lead" } },
  capability_gaps: gaps,
} as unknown as OrgState;

describe("gap intake helpers", () => {
  it("polls only gaps the driver can still act on, oldest first", () => {
    expect(liveGaps(state).map((g) => g.gap_id)).toEqual(["g_acq", "g_open"]);
    expect(liveGaps(null)).toEqual([]);
  });

  it("says what automatic acquisition is doing, without claiming success", () => {
    const open = gaps.g_open;
    expect(intakeDetail(open, undefined)).toBeNull();
    expect(intakeDetail(open, status("g_open", { enabled: false, state: "disabled" }))).toMatch(/off on this server/);
    expect(intakeDetail(open, status("g_open", {}))).toBe("queued for automatic acquisition");
    expect(intakeDetail(open, status("g_open", { state: "waiting", code: "provisioning" }))).toMatch(/role to become active/);
    const acq = gaps.g_acq;
    expect(intakeDetail(acq, status("g_acq", { state: "acquiring" }))).toMatch(/acq-1 · generating/);
    expect(intakeDetail(acq, status("g_acq", { state: "attention", code: "retry_read" }))).toMatch(/retry it explicitly/);
    // Acquired or later: the kernel lifecycle label stays.
    expect(intakeDetail(gaps.g_done, status("g_done", { state: "installed" }))).toBeNull();
    const rows = gapsForRole(state, "role_a", { g_open: status("g_open", {}) });
    expect(rows.find((r) => r.gapId === "g_open")?.detail).toBe("queued for automatic acquisition");
    expect(rows.find((r) => r.gapId === "g_acq")?.detail).toBe("acquisition acq-1");
  });

  it("raises attention only for blocked or waiting live gaps, with the right kind", () => {
    const items = intakeAttention(state, {
      g_acq: status("g_acq", { state: "attention", code: "unknown_outcome", acquisition_id: "acq-1" }),
      g_open: status("g_open", { state: "waiting", code: "provisioning" }),
      g_done: status("g_done", { state: "attention", code: "quota" }),
    });
    expect(items.map((i) => [i.key, i.kind])).toEqual([
      ["gap-intake:g_acq:unknown_outcome", "unknown"],
      ["gap-intake:g_open:provisioning", "waiting"],
    ]);
    expect(items[0]!.refs).toEqual(["g_acq", "acq-1"]);
    expect(items[0]!.detail).toMatch(/nothing is replayed · requested by Lead/);
    expect(intakeAttention(state, { g_open: status("g_open", {}) })).toEqual([]);
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

it("polls live gaps and drops responses from another organization or gap", async () => {
  dom = new JSDOM("<div id='root'></div>");
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const calls: { org: string; path: string; resolve: (s: GapIntakeStatus) => void }[] = [];
  const api = (org: string) =>
    ({
      get: (path: string) => new Promise<GapIntakeStatus>((resolve) => calls.push({ org, path, resolve })),
    }) as unknown as Api;
  const a = api("a");
  const b = api("b");
  function Probe({ api }: { api: Api }) {
    return <pre>{JSON.stringify(useGapIntake(api, state))}</pre>;
  }
  const container = dom.window.document.getElementById("root")!;
  root = createRoot(container);
  await act(async () => root!.render(<StrictMode><Probe api={a} /></StrictMode>));
  const fromA = calls.filter((c) => c.org === "a");
  expect(new Set(fromA.map((c) => c.path))).toEqual(
    new Set(["/capability-gaps/g_acq/acquisition", "/capability-gaps/g_open/acquisition"]),
  );
  // The live poll after StrictMode's remount answers; a mislabeled body is ignored.
  const live = fromA.slice(-2);
  await act(async () => {
    for (const c of live) {
      const id = c.path.split("/")[2]!;
      c.resolve(status(id, { state: id === "g_acq" ? "acquiring" : "ready" }));
    }
  });
  expect(Object.keys(JSON.parse(container.textContent!)).sort()).toEqual(["g_acq", "g_open"]);
  await act(async () => fromA[0]!.resolve(status("g_other", {})));
  expect(JSON.parse(container.textContent!).g_other).toBeUndefined();

  // Switching organizations clears and ignores the old organization's replies.
  const pendingA = calls.length;
  await act(async () => root!.render(<StrictMode><Probe api={b} /></StrictMode>));
  expect(JSON.parse(container.textContent!)).toEqual({});
  await act(async () => fromA[1]!.resolve(status("g_open", { state: "attention", code: "quota" })));
  expect(JSON.parse(container.textContent!)).toEqual({});
  expect(calls.slice(pendingA).every((c) => c.org === "b")).toBe(true);
});
