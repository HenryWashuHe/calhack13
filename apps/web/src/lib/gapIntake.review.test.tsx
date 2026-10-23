// Review regressions for PR71. Safe expected assertions; no live requests.
import { afterEach, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JSDOM } from "jsdom";
import type { Api } from "./api";
import type { CapabilityGap, OrgState } from "./contracts";
import { INTAKE_POLL_MS, intakeDetail, useGapIntake, type GapIntakeStatus } from "./gapIntake";

const gap = {
  gap_id: "gap-a", role_id: "role-a", opened_in_revision: 1,
  status: { state: "acquiring", acquisition_id: "acq-a" },
} as CapabilityGap;
const state = { capability_gaps: { "gap-a": gap } } as unknown as OrgState;
function status(extra: Partial<GapIntakeStatus>): GapIntakeStatus {
  return { gap_id: "gap-a", enabled: true, state: "acquiring", code: null,
    acquisition_id: "acq-a", updated_at: null, ...extra };
}
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

it("a delayed old poll cannot restore attention after a newer poll cleared it", async () => {
  dom = new JSDOM("<div id='root'></div>");
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
  const calls: Array<(response: GapIntakeStatus) => void> = [];
  const api = { get: () => new Promise<GapIntakeStatus>((resolve) => calls.push(resolve)) } as unknown as Api;
  function Probe() { return <pre>{JSON.stringify(useGapIntake(api, state))}</pre>; }
  const container = dom.window.document.getElementById("root")!;
  root = createRoot(container);
  await act(async () => root!.render(<Probe />));
  expect(calls).toHaveLength(1);
  await act(async () => { vi.advanceTimersByTime(INTAKE_POLL_MS); });
  expect(calls).toHaveLength(2);
  await act(async () => calls[1]!(status({ updated_at: "2026-10-22T22:00:05Z" })));
  expect(JSON.parse(container.textContent!)["gap-a"].state).toBe("acquiring");
  await act(async () => calls[0]!(status({ state: "attention", code: "quota", updated_at: "2026-10-22T22:00:00Z" })));
  expect(JSON.parse(container.textContent!)["gap-a"].state).toBe("acquiring");
  expect(JSON.parse(container.textContent!)["gap-a"].code).toBeNull();
});

it("an existing acquiring gap reports disabled automatic intake after server restart", () => {
  const detail = intakeDetail(gap, status({ enabled: false, state: "disabled" }));
  expect(detail).toMatch(/off on this server|disabled|paused/i);
  expect(detail).not.toMatch(/generating, building and independently verifying/);
});
