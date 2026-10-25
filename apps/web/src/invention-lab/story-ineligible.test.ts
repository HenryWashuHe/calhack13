// Review guard: a trial reads "ineligible" when any of its runs fails the
// process-slot rule, not only when every run does. The matrix already marks
// the single run; the story must not tally that trial by its verdict.
import { describe, expect, it } from "vitest";
import type { Trial } from "./lineage";
import { USAGE_DIMENSIONS } from "./records";
import { verdictWords } from "./story";

const measured = { kind: "measured", value: 0 } as const;
const unknown = { kind: "unknown" } as const;
function run(outcome: string, modelCalls: typeof measured | typeof unknown) {
  const usage = Object.fromEntries(USAGE_DIMENSIONS.map((d) => [d, measured]));
  return { receipt: { outcome, usage: { ...usage, model_calls: modelCalls } } };
}
function trial(verdict: string | null, executions: unknown[]): Trial {
  return { plan: {}, verdict: verdict ? { verdict } : null, executions } as unknown as Trial;
}

describe("story verdict words under the process-slot rule", () => {
  it("reads a trial as ineligible when only its candidate run was not measured", () => {
    const t = trial("rejected", [run("output_generated", unknown), run("output_generated", measured)]);
    expect(verdictWords([t])).toEqual(["ineligible"]);
  });
  it("reads a trial as ineligible when only one run failed to execute", () => {
    const t = trial("accepted", [run("output_generated", measured), run("failed", measured)]);
    expect(verdictWords([t])).toEqual(["ineligible"]);
  });
  it("keeps the recorded verdict when every run is eligible, and says so when there is none", () => {
    const eligible = trial("accepted", [run("output_generated", measured), run("output_generated", measured)]);
    const unscored = trial(null, [run("output_generated", measured)]);
    const empty = trial("rejected", []);
    expect(verdictWords([eligible, unscored, empty])).toEqual(["accepted", "no verdict", "rejected"]);
  });
});
