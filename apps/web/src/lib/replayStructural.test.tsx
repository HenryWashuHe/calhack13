// Replay must advance while new structural generations keep arriving.
// Follow-up to the #110 review: appending a generation every second used to
// change the generation sequence and restart the 2.4 s step deadline forever.
import { afterEach, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JSDOM } from "jsdom";
import { OrgPanel } from "../components/OrgPanel";
import type { Api } from "./api";
import type { DomainEvent, OrgState } from "./contracts";

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

const event = (seq: number, revision: number, type: string, payload: object) =>
  ({
    seq, revision, type, payload, org_id: "org_a", mission_id: `mission-${revision}`,
    schema_version: 1, event_id: `event-${seq}`, timestamp: "2026-10-23T00:00:00Z",
  }) as DomainEvent;

const mission = (revision: number) =>
  event(revision, revision, "mission_created", { mission_id: `mission-${revision}`, goal: `Run event ${revision}` });

function mount() {
  dom = new JSDOM("<div id='root'></div>");
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  const container = dom.window.document.getElementById("root")!;
  root = createRoot(container);
  return container;
}

function stateAt(revision: number) {
  return {
    org_id: "org_a", name: "A", revision,
    roles: {}, responsibilities: {}, capability_gaps: {},
  } as unknown as OrgState;
}

function apiFor(events: () => DomainEvent[], last: () => number) {
  return {
    get: async (path: string) => {
      if (path === "/organization/audit") return {
        org_id: "org_a", snapshots_checked: last() + 1, first_revision: 0, last_revision: last(),
        events_checked: events().length, invariants: [], violation_count: 0, violations: [],
      };
      return { ...stateAt(last()), revision: Number(path.split("/").at(-1)) };
    },
  } as unknown as Api;
}

const banner = (container: HTMLElement) => container.querySelector(".history-banner.replay")?.textContent ?? "";

it("replay advances while a structural generation is appended every second", async () => {
  const container = mount();
  let events = [mission(1), mission(2)];
  let state = stateAt(2);
  const api = apiFor(() => events, () => state.revision);
  const render = () => root!.render(<OrgPanel api={api} state={state} changes={[]} events={events} />);
  await act(async () => render());
  const replay = [...container.querySelectorAll("button")].find((b) => b.textContent === "▶ Replay")!;
  await act(async () => replay.click());
  expect(banner(container)).toContain("r1");
  for (let n = 0; n < 3; n++) {
    await act(async () => { vi.advanceTimersByTime(1000); });
    const revision = 3 + n;
    events = [...events, mission(revision)];
    state = stateAt(revision);
    await act(async () => render());
  }
  expect(banner(container)).toContain("r2");
});

it("replay continues into generations that arrived while it was running", async () => {
  const container = mount();
  let events = [mission(1), mission(2)];
  let state = stateAt(2);
  const api = apiFor(() => events, () => state.revision);
  const render = () => root!.render(<OrgPanel api={api} state={state} changes={[]} events={events} />);
  await act(async () => render());
  const replay = [...container.querySelectorAll("button")].find((b) => b.textContent === "▶ Replay")!;
  await act(async () => replay.click());
  // r3 arrives during the first step; replay should reach it rather than stop at r2.
  await act(async () => { vi.advanceTimersByTime(1000); });
  events = [...events, mission(3)];
  state = stateAt(3);
  await act(async () => render());
  await act(async () => { vi.advanceTimersByTime(1400); });
  expect(banner(container)).toContain("r2");
  await act(async () => { vi.advanceTimersByTime(2400); });
  expect(banner(container)).toContain("r3");
  await act(async () => { vi.advanceTimersByTime(2400); });
  expect(banner(container)).toBe("");
});
