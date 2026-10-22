// Henry's Claude: the capability-gap loop reads as a story in the evidence log.
import { describe as suite, expect, it } from "vitest";
import type { DomainEvent, OrgState } from "./contracts";
import { describe } from "./describe";

const base = {
  schema_version: 1, seq: 1, event_id: "e", org_id: "org_a", revision: 1, timestamp: "2026-10-22T00:00:00Z",
};
const state = { roles: { role_v: { role_id: "role_v", name: "Venue operations" } } } as unknown as OrgState;
const ev = (e: object) => ({ ...base, ...e }) as DomainEvent;
const cap = { capability_id: "venue-portal-reader", version: "1" };

suite("capability gap events", () => {
  it("names the capability and the role it was requested for", () => {
    const d = describe(
      ev({ type: "capability_gap_opened", payload: { gap_id: "gap_1", requirement_id: "req", role_id: "role_v", responsibility_id: "rsp", capability_id: "venue-portal-reader" } }),
      state,
    );
    expect(d.title).toBe("Capability requested");
    expect(d.detail).toBe("venue-portal-reader for Venue operations");
    expect(d.category).toBe("capabilities");
    expect(d.refs).toEqual(["gap_1"]);
  });

  it("shows each transition with the lifecycle labels, never verified as installed", () => {
    const acquired = { state: "acquired", acquisition_id: "acq", capability: cap, publication_ref: null };
    const verified = describe(ev({ type: "capability_gap_status_changed", payload: { gap_id: "gap_1", from: { state: "acquiring", acquisition_id: "acq" }, to: acquired } }), state);
    expect(verified.title).toBe("Requested capability verified");
    expect(verified.detail).toBe("ACQUIRING → VERIFIED · venue-portal-reader@1 · verified; not installed here yet");
    expect(verified.tone).toBe("warn");
    const installed = describe(
      ev({ type: "capability_gap_status_changed", payload: { gap_id: "gap_1", from: acquired, to: { state: "installed", acquisition_id: "acq", capability: cap, installation_id: "inst_1" } } }),
      state,
    );
    expect(installed.title).toBe("Requested capability installed");
    expect(installed.detail).toBe("VERIFIED → INSTALLED · venue-portal-reader@1 · installation inst_1");
    expect(installed.tone).toBe("ok");
    const refused = describe(
      ev({ type: "capability_gap_status_changed", payload: { gap_id: "gap_1", from: { state: "open" }, to: { state: "refused", reason: "no policy", acquisition_id: null } } }),
      state,
    );
    expect(refused.detail).toBe("REQUESTED → REFUSED · no policy");
    expect(refused.tone).toBe("bad");
  });

  it("labels the kernel note that wakes the planner, and only that one", () => {
    const note = describe(ev({ type: "observation_recorded", payload: { observation_id: "obs", source: "kernel.capability_gap", summary: "Requested capability venue-portal-reader@1 is installed" } }), state);
    expect(note.title).toBe("Planner woken: requested capability is final");
    expect(note.detail).toContain("venue-portal-reader@1 is installed");
    const portal = describe(ev({ type: "observation_recorded", payload: { observation_id: "o2", source: "portal", summary: "closed" } }), state);
    expect(portal.title).toBe("Observation");
  });
});
