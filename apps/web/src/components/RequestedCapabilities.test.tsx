// Henry's Claude: a requested capability's chip shortens a 64-hex version in
// its label, so the chip itself carries the full version as its hover text,
// like attached capability chips do (Codex's note on #88).
import { renderToStaticMarkup } from "react-dom/server";
import { expect, it } from "vitest";
import type { Api } from "../lib/api";
import type { OrgState } from "../lib/contracts";
import { OrgPanel } from "./OrgPanel";

it("shows the full version on hover when the chip label is shortened", () => {
  const version = "a".repeat(64);
  const state = {
    org_id: "org_a", name: "A", revision: 3,
    roles: { role_a: { role_id: "role_a", name: "Lead", charter: "", state: "ACTIVE", authority: { scopes: [], budget_cents: 0 } } },
    responsibilities: {},
    capability_gaps: {
      g1: {
        gap_id: "g1", mission_id: "m", requirement_id: "r", requirement_digest: "d", triggering_evidence: ["o"],
        role_id: "role_a", responsibility_id: "rsp", capability_id: "venue-reader", purpose: "Read venues",
        observed_sources: ["https://portal.example/a"], opened_in_revision: 1,
        status: { state: "acquired", acquisition_id: "acq_1", capability: { capability_id: "venue-reader", version } },
      },
    },
  } as unknown as OrgState;
  const html = renderToStaticMarkup(<OrgPanel api={{} as Api} state={state} changes={[]} />);
  expect(html).toContain(`venue-reader@aaaaaaaa…`);
  expect(html).toContain(`title="venue-reader@${version}"`);
});
