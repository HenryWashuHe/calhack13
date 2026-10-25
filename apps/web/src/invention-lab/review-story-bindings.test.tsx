// Independent #149 review: retained records and static React rendering only.
// Never runs the exporter, generated methods, providers or evaluators.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ResultsBoard } from "./ResultsBoard";
import { buildLineage } from "./lineage";
import { buildMatrix } from "./matrix";
import { decodeExecutionReceipt, decodeGenerationStatus, decodeRevisionObservation } from "./records";
import { story } from "./story";
import aStatus from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/final-status.json";
import bStatus from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/final-status.json";
import revision from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/revision-observation.json";
import hiddenReceipt from "../../../../docs/invention-lab/evidence/selective-transfer-20261024/hidden-2/ravine-crossing/run/candidate/receipt.json";

function revised(status = bStatus) {
  return buildLineage({
    generations: [aStatus, status].map((s) => ({ status: decodeGenerationStatus(s) })),
    revisions: [decodeRevisionObservation(revision)],
  });
}

describe("PR149: revision request and unknown panel claims", () => {
  it("retains the original matching revision's measured cost", () => {
    expect(story(revised(), aStatus.org_id, bStatus.org_id)[2]!.text).toContain("1 model calls");
  });

  it("does not bill a different request as the revision's own generation record", () => {
    const different = structuredClone(bStatus);
    different.request_sha256 = "e".repeat(64);
    different.usage.model_calls.value = 99;
    expect(different.request_sha256).not.toBe(revision.request_sha256);
    const l = revised(different);
    expect(story(l, aStatus.org_id, bStatus.org_id)[2]!.text).not.toContain("99 model calls");
    expect(l.problems.length).toBeGreaterThan(0);
  });

  it("does not describe an unestablished panel as public development", () => {
    // Only this original hidden-run receipt is supplied, not its plan. The
    // decoder correctly reports unknown; the visible board must retain that.
    const l = buildLineage({ generations: [], executions: [decodeExecutionReceipt(hiddenReceipt)] });
    const run = [...buildMatrix(l).rows[0]!.cells.values()][0]![0]!;
    expect(run.panel).toBe("unknown");
    const html = renderToStaticMarkup(<ResultsBoard lineage={l} orgA={aStatus.org_id} orgB={bStatus.org_id} />);
    const visible = html.split("All records behind this board")[0]!.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");
    expect(visible).not.toContain("the others on public development instances");
    expect(visible).toMatch(/panel (?:not established|unknown)/i);
  });
});
