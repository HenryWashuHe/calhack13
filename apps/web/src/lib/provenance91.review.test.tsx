// Independent exact-source PR91 regressions. All HTTP is local stub data.
import { afterEach, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JSDOM } from "jsdom";
import { OrgPanel } from "../components/OrgPanel";
import { verdicts, type HistoryAudit } from "./audit";
import type { Api } from "./api";
import type { DomainEvent, OrgState } from "./contracts";

it("does not mark a missing snapshot revision as having passed an audit", () => {
  // audit_snapshot on rows r0,r1,r3 reports the gap at r3 (the later row).
  // No r2 was loaded or checked, and there is no separate violation at r2.
  const report: HistoryAudit = {
    org_id: "org_a", snapshots_checked: 3, first_revision: 0, last_revision: 3,
    events_checked: 0, invariants: ["snapshot_history_contiguous"],
    violation_count: 1,
    violations: [{ revision: 3, invariant: "snapshot_history_contiguous", detail: "r3 follows r1" }],
  };
  expect(verdicts(report).find((row) => row.revision === 2)?.verdict).not.toBe("ok");
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

it.each([
  { label: "while the event stream is idle", busy: false },
  { label: "while unrelated job events keep the organization busy", busy: true },
])("replay advances $label", async ({ busy }) => {
  dom = new JSDOM("<div id='root'></div>");
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  const container = dom.window.document.getElementById("root")!;
  root = createRoot(container);
  const state = {
    org_id: "org_a", name: "A", revision: 2,
    roles: {}, responsibilities: {}, capability_gaps: {},
  } as unknown as OrgState;
  const event = (seq: number, revision: number, type: string, payload: object) => ({
    seq, revision, type, payload, org_id: "org_a", mission_id: "mission-a",
    schema_version: 1, event_id: `event-${seq}`, timestamp: "2026-10-23T00:00:00Z",
  }) as DomainEvent;
  let events = [
    event(1, 1, "mission_created", { mission_id: "mission-a", goal: "Run an event" }),
    event(2, 2, "mission_created", { mission_id: "mission-b", goal: "Run a second event" }),
  ];
  const api = { get: async (path: string) => {
    if (path === "/organization/audit") return {
      org_id: "org_a", snapshots_checked: 3, first_revision: 0, last_revision: 2,
      events_checked: events.length, invariants: [], violation_count: 0, violations: [],
    };
    return { ...state, revision: Number(path.split("/").at(-1)) };
  } } as unknown as Api;
  const render = () => root!.render(<OrgPanel api={api} state={state} changes={[]} events={events} />);
  await act(async () => render());
  const replay = [...container.querySelectorAll("button")].find((button) => button.textContent === "▶ Replay")!;
  await act(async () => replay.click());
  expect(container.querySelector(".history-banner.replay")?.textContent).toContain("r1");
  // No structural generation changes. SSE job events still replace events[]
  // and the live hook refreshes changes[]/state; replay's deadline must survive.
  for (let n = 0; n < 3; n++) {
    await act(async () => { vi.advanceTimersByTime(1000); });
    if (busy) {
      events = [...events, event(3 + n, 2, "job_status_changed", {
        job_id: `job-${n}`, from: "RUNNING", to: "VERIFYING",
      })];
      await act(async () => render());
    }
  }
  expect(container.querySelector(".history-banner.replay")?.textContent).toContain("r2");
});
