// Independent PR93/94 review regressions. These are synthetic UI records,
// not execution, provider, adoption or improvement evidence.
import { describe, expect, it } from "vitest";
import { adoptionLabel, phaseLabel } from "./claims";

describe("PR93: all unrecognized enum strings have visible labels", () => {
  it("does not resolve prototype property names as labels", () => {
    for (const label of [phaseLabel, adoptionLabel]) {
      for (const raw of ["constructor", "toString", "__proto__"]) {
        expect.soft(label(raw), `${label.name}(${raw})`).toMatchObject({
          text: `unrecognized: ${raw}`,
          tone: "neutral",
          limit: expect.any(String),
        });
      }
    }
  });

  it("retains ordinary known and unrecognized labels", () => {
    expect(phaseLabel("failed").text).toBe("generation failed");
    expect(adoptionLabel("unknown").text).toBe("unknown — retained, not concluded");
    expect(phaseLabel("unlisted-phase").text).toBe("unrecognized: unlisted-phase");
    expect(adoptionLabel("unlisted-outcome").text).toBe("unrecognized: unlisted-outcome");
  });
});
