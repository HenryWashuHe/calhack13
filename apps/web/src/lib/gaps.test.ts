import { describe, expect, it } from "vitest";
import type { CapabilityGap, OrgState } from "./contracts";
import { gapLabel, gapsForRole } from "./gaps";

const cap = { capability_id: "listing-reader", version: "1" };
const gap = (id: string, role: string, rev: number, status: CapabilityGap["status"]): CapabilityGap =>
  ({
    gap_id: id,
    mission_id: "m",
    requirement_id: "r",
    requirement_digest: "d",
    triggering_evidence: ["o"],
    role_id: role,
    responsibility_id: "rsp",
    capability_id: "listing-reader",
    purpose: "Read listings",
    observed_sources: ["https://portal.example/a"],
    status,
    opened_in_revision: rev,
  }) as CapabilityGap;

describe("capability gaps in the console", () => {
  it("labels every lifecycle state and never calls verified installed", () => {
    expect(gapLabel({ state: "open" }).label).toBe("REQUESTED");
    expect(gapLabel({ state: "acquiring", acquisition_id: "acq-1" }).detail).toContain("acq-1");
    const verified = gapLabel({ state: "acquired", acquisition_id: "acq-1", capability: cap, publication_ref: "registry:x" });
    expect(verified.label).toBe("VERIFIED");
    expect(verified.detail).toMatch(/not installed/);
    expect(verified.version).toBe("listing-reader@1");
    const installed = gapLabel({ state: "installed", acquisition_id: "acq-1", capability: cap, installation_id: "inst_1" });
    expect(installed).toMatchObject({ label: "INSTALLED", tone: "ok", version: "listing-reader@1" });
    expect(gapLabel({ state: "refused", reason: "no policy", acquisition_id: null })).toMatchObject({ label: "REFUSED", tone: "bad", detail: "no policy" });
  });

  it("lists only the role's gaps, oldest first", () => {
    const state = {
      capability_gaps: {
        g2: gap("g2", "role_a", 5, { state: "open" }),
        g1: gap("g1", "role_a", 3, { state: "acquiring", acquisition_id: "acq" }),
        g3: gap("g3", "role_b", 1, { state: "open" }),
      },
    } as unknown as OrgState;
    expect(gapsForRole(state, "role_a").map((g) => g.gapId)).toEqual(["g1", "g2"]);
    expect(gapsForRole(state, "role_c")).toEqual([]);
    expect(gapsForRole(null, "role_a")).toEqual([]);
    expect(gapsForRole({} as OrgState, "role_a")).toEqual([]);
  });
});
