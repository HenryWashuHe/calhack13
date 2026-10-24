// Henry's Claude: the adoption decision trail. Outcome first; candidates only
// on demand; an unreadable log never reads as "no candidates"; evidence tiers
// this view can't establish say so. jsdom, stub API.
import { afterEach, expect, it } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
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

it("shows the conclusion, then each candidate's decision and local acceptance on demand", async () => {
  const container = setup();
  const calls: string[] = [];
  const api = {
    get: (path: string) => {
      calls.push(path);
      return Promise.resolve({
        adoption_id: "adopt_1",
        items: [
          row(0, { publisher: "org-b", decision: "own_publication", reason: "published by this organization" }),
          row(1, { decision: "not_applicable", reason: rejected.reason, acceptance_ref: rejected.candidate!.acceptance_ref }),
        ],
      });
    },
  } as unknown as Api;
  await act(async () => root!.render(<AdoptionDecisions api={api} adoptionId="adopt_1" outcome={rejected} />));
  expect(container.textContent).toContain("adoption: not applicable");
  expect(container.textContent).toContain("venue-reader@1.0.0 from org-a");
  expect(container.textContent).toContain("1 of 2 cases failed");
  expect(calls).toEqual([]); // nothing fetched until asked
  const button = container.querySelector("button")!;
  await act(async () => button.dispatchEvent(new dom!.window.MouseEvent("click", { bubbles: true })));
  expect(calls).toEqual(["/capability-adoptions/adopt_1/candidates"]);
  const text = container.textContent!;
  expect(text).toContain("own publication");
  expect(text).toContain("rejected by local checks");
  expect(text).toContain("adoption-rejection:adopt_1:1.0.0");
  expect(text).toContain("failed (recorded)");
  expect(text).toContain("task success");
  expect(text).toContain("not measured");
  expect(text).toContain("live vs fixture not recorded");
  expect(text.toLowerCase()).not.toContain("improved");
});

it("an unreadable decision log is not shown as no candidates", async () => {
  const container = setup();
  const api = { get: () => Promise.reject(new Error("500")) } as unknown as Api;
  await act(async () => root!.render(<AdoptionDecisions api={api} adoptionId="adopt_1" outcome={rejected} />));
  await act(async () =>
    container.querySelector("button")!.dispatchEvent(new dom!.window.MouseEvent("click", { bubbles: true })),
  );
  expect(container.textContent).toContain("couldn't be read");
  expect(container.textContent).not.toContain("No candidates were recorded");
});

it("a response for another adoption is ignored", async () => {
  const container = setup();
  const api = {
    get: () => Promise.resolve({ adoption_id: "adopt_other", items: [row(0, { decision: "adopted" })] }),
  } as unknown as Api;
  await act(async () => root!.render(<AdoptionDecisions api={api} adoptionId="adopt_1" outcome={rejected} />));
  await act(async () =>
    container.querySelector("button")!.dispatchEvent(new dom!.window.MouseEvent("click", { bubbles: true })),
  );
  expect(container.textContent).toContain("Loading decisions");
  expect(container.textContent).not.toContain("adopted");
});

it("a refused gap's row in the organization tree shows the adoption conclusion", () => {
  const state = {
    org_id: "org_b", name: "B", revision: 3,
    roles: { role_b: { role_id: "role_b", name: "Verifier", charter: "", state: "ACTIVE", authority: { scopes: [], budget_cents: 0 } } },
    responsibilities: {},
    capability_gaps: {
      g1: {
        gap_id: "g1", mission_id: "m", requirement_id: "r", requirement_digest: "d", triggering_evidence: ["o"],
        role_id: "role_b", responsibility_id: "rsp", capability_id: "venue-reader", purpose: "Read venues",
        observed_sources: ["https://portal.example/b"], opened_in_revision: 1,
        status: { state: "refused", reason: "Not applicable here", acquisition_id: "adopt_1" },
      },
    },
  } as unknown as OrgState;
  const adoption: Record<string, GapAdoptionStatus> = {
    g1: {
      gap_id: "g1", enabled: true, state: "refused", code: "rejected", adoption_id: "adopt_1",
      synthesis_fallback: false, selected: null, installation_id: null, updated_at: null, outcome: rejected,
    },
  };
  const html = renderToStaticMarkup(
    <GapAdoptionContext.Provider value={adoption}>
      <OrgPanel api={{} as Api} state={state} changes={[]} />
    </GapAdoptionContext.Provider>,
  );
  expect(html).toContain("adoption: not applicable");
  expect(html).toContain("Decisions (2)");
});
