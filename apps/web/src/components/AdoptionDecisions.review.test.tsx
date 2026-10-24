// Independent PR15 regressions: revision isolation, request ordering, and bounded discovery.
// jsdom and stub APIs only; no live provider execution.
import { afterEach, expect, it } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JSDOM } from "jsdom";
import { vi } from "vitest";
import type { Api } from "../lib/api";
import type { OrgState } from "../lib/contracts";
import type { CandidateDecision, OutcomeView } from "../lib/adoptionDecisions";
import { GapAdoptionContext, type GapAdoptionStatus } from "../lib/gapAdoption";
import { AdoptionDecisions } from "./AdoptionDecisions";
import { OrgPanel } from "./OrgPanel";

let root: Root | undefined;
let dom: JSDOM | undefined;
afterEach(() => {
  act(() => root?.unmount());
  root = undefined;
  dom?.window.close();
  dom = undefined;
  vi.unstubAllGlobals();
});
function setup() {
  dom = new JSDOM("<div id='root'></div>");
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const container = dom.window.document.getElementById("root")!;
  root = createRoot(container);
  return container;
}

const rejected: OutcomeView = {
  outcome: "not_applicable",
  reason: "this organization's acceptance suite rejected it: 1 of 2 cases failed (case-b)",
  considered: 2,
  decisions: { not_applicable: 1, own_publication: 1 },
  candidate: {
    capability_id: "venue-reader", version: "1.0.0", publisher: "org-a",
    decision: "not_applicable", acceptance_ref: "adoption-rejection:adopt_1:1.0.0",
  },
};
const row = (rank: number, extra: Partial<CandidateDecision>): CandidateDecision => ({
  rank, score: 2 - rank, capability_id: "venue-reader", version: "1.0.0", publisher: "org-a",
  source_digest: "d", applicability_conditions: [], required_permissions: [], decision: "not_evaluated",
  reason: "r", missing_scopes: [], acceptance_ref: null, decided_at: "2026-10-24T00:00:00Z", ...extra,
});

it("independent: reopening decisions cannot let an older request overwrite the latest evidence", async () => {
  const container = setup();
  const calls: Array<(v: unknown) => void> = [];
  const api = { get: () => new Promise((resolve) => calls.push(resolve)) } as unknown as Api;
  await act(async () => root!.render(<AdoptionDecisions api={api} adoptionId="adopt_1" outcome={rejected} />));
  const click = async () => act(async () => container.querySelector("button")!.click());
  await click(); await click(); await click();
  expect(calls).toHaveLength(2);
  await act(async () => calls[1]!({ adoption_id: "adopt_1", items: [row(0, { decision: "not_applicable", reason: "latest local rejection" })] }));
  expect(container.textContent).toContain("latest local rejection");
  await act(async () => calls[0]!({ adoption_id: "adopt_1", items: [row(0, { decision: "selected", reason: "stale selected evidence" })] }));
  expect(container.textContent).toContain("latest local rejection");
  expect(container.textContent).not.toContain("stale selected evidence");
});

it("independent: no-candidate tooltip describes only bounded discovery", async () => {
  const container = setup();
  const outcome: OutcomeView = { outcome: "no_candidate", reason: "no eligible candidate was found in the bounded registry search", considered: 0, decisions: {}, candidate: null };
  await act(async () => root!.render(<AdoptionDecisions api={null} adoptionId={null} outcome={outcome} />));
  expect(container.querySelector(".chip")!.getAttribute("title")).not.toContain("nothing eligible was published");
});

it("independent: historical revision does not display a later live adoption result", async () => {
  const container = setup();
  const cap = { capability_id: "shared-reader", version: "1.0.0" };
  const g = { gap_id: "g1", mission_id: "m", requirement_id: "r", requirement_digest: "d", triggering_evidence: ["o"], role_id: "role_b", responsibility_id: "rsp", capability_id: "reader", purpose: "Read inputs", observed_sources: ["https://fixture.example/input"], opened_in_revision: 1 };
  const state = { org_id: "org_b", name: "B", revision: 2, roles: { role_b: { role_id: "role_b", name: "Verifier", charter: "", state: "ACTIVE", authority: { scopes: [], budget_cents: 0 } } }, responsibilities: {}, capability_gaps: { g1: { ...g, status: { state: "installed", acquisition_id: "adopt_1", capability: cap, installation_id: "i1" } } } } as unknown as OrgState;
  const earlier = { ...state, revision: 1, capability_gaps: { g1: { ...g, status: { state: "open" } } } } as unknown as OrgState;
  const adopted: OutcomeView = { outcome: "adopted", reason: "passed local checks", considered: 1, decisions: { adopted: 1 }, candidate: { ...cap, publisher: "org-a", decision: "adopted", acceptance_ref: "acceptance:late" } };
  const statuses = { g1: { gap_id: "g1", enabled: true, state: "installed", code: null, adoption_id: "adopt_1", synthesis_fallback: false, selected: cap, installation_id: "i1", updated_at: null, outcome: adopted } } as Record<string, GapAdoptionStatus>;
  const events = [1,2].map((rev) => ({ seq: rev, revision: rev, type: "mission_created", payload: { mission_id: `m${rev}`, goal: `goal ${rev}` }, org_id: "org_b", mission_id: `m${rev}`, schema_version: 1, event_id: `e${rev}`, timestamp: "2026-10-24T00:00:00Z" })) as import("../lib/contracts").DomainEvent[];
  const api = { get: async (path: string) => path === "/organization/audit" ? { org_id: "org_b", snapshots_checked: 2, first_revision: 1, last_revision: 2, events_checked: 2, invariants: [], violation_count: 0, violations: [] } : earlier } as unknown as Api;
  await act(async () => root!.render(<GapAdoptionContext.Provider value={statuses}><OrgPanel api={api} state={state} changes={[]} events={events} /></GapAdoptionContext.Provider>));
  expect(container.textContent).toContain("adoption: adopted");
  const replay = [...container.querySelectorAll("button")].find((button) => button.textContent === "▶ Replay")!;
  expect(replay).toBeTruthy();
  await act(async () => replay.click());
  expect(container.querySelector(".history-banner.replay")?.textContent).toContain("r1");
  expect(container.textContent).toContain("REQUESTED");
  expect(container.textContent).not.toContain("adoption: adopted");
});
