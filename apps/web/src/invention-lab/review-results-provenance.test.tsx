// Independent #148/#149 review. Retained JSON and synthetic substitutions only;
// no generated source, providers, scorer, executor, browser or exporter runs.
import fs from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ResultsBoard } from "./ResultsBoard";
import { canonicalSha256 } from "./digest";
import { buildLineage } from "./lineage";
import { buildMatrix, cellText } from "./matrix";
import { decodeEvaluationReceipt, decodeExecutionReceipt, decodeGenerationStatus, decodeMetricsArtifact, decodeRevisionObservation } from "./records";
import { story } from "./story";
import execution from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/candidate/receipt.json";
import evaluation from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/candidate-score/evaluation.json";
import aStatus from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/final-status.json";
import bStatus from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/final-status.json";
import revision from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/revision-observation.json";

const bytes = new Uint8Array(fs.readFileSync(new URL(
  "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/candidate-score/artifacts/db11d4ff7ed356ee1ff084f185a0e7483fab71d1147892d38d2abc59d19ddc60",
  import.meta.url,
)));
const metrics = decodeMetricsArtifact(bytes);
const A = aStatus.org_id;
const B = bStatus.org_id;
function scored(evaluations = [decodeEvaluationReceipt(evaluation)], artifacts = [metrics]) {
  return buildLineage({ generations: [], executions: [decodeExecutionReceipt(execution)], evaluations, metrics: artifacts });
}
const runs = (lineage: ReturnType<typeof buildLineage>) => buildMatrix(lineage).rows[0]!.cells.get("ravine-crossing")!;

describe("PR148: scoring evidence boundaries", () => {
  it("keeps the retained exact scoring artifact's 1/6 result", () => {
    expect(cellText(runs(scored()))).toBe("1/6");
  });

  it("does not treat an unknown evaluation as a scored result missing metrics", () => {
    const unknown = decodeEvaluationReceipt({ ...evaluation, outcome: { kind: "unknown", reason: "interrupted" } });
    const result = runs(scored([unknown]));
    expect(result[0]!.successes).toBeNull();
    expect(cellText(result)).not.toBe("metrics not supplied");
  });

  it("does not count unsupported metrics schemas merely because their digest matches", () => {
    const other = structuredClone(evaluation);
    other.outcome.metrics.schema = "foreign-metric/99";
    const result = runs(scored([decodeEvaluationReceipt(other)]));
    expect(result.every((run) => run.successes === null)).toBe(true);
  });

  it("keeps conflicting evaluations visible instead of choosing by file order", () => {
    const changed = JSON.parse(new TextDecoder().decode(bytes));
    for (const row of changed.instances) row.success = true;
    changed.success_count = changed.instances.length;
    const alternative = decodeMetricsArtifact(new TextEncoder().encode(JSON.stringify(changed)));
    const second = structuredClone(evaluation);
    second.request.request_id = "second-scoring-request";
    second.request_sha256 = canonicalSha256({ domain: "genesis.development_evaluation_request", version: 1, record: second.request });
    second.outcome.metrics.sha256 = alternative.sha256;
    const first = decodeEvaluationReceipt(evaluation);
    const later = decodeEvaluationReceipt(second);
    for (const ordered of [[first, later], [later, first]]) {
      const lineage = scored(ordered, [metrics, alternative]);
      const results = runs(lineage);
      const values = results.map((r) => r.successes);
      expect(lineage.problems.length > 0 || results.every((r) => r.successes === null) || (values.includes(1) && values.includes(6))).toBe(true);
    }
  });
});

function revisionStory(extra: ReturnType<typeof decodeGenerationStatus>[] = [], withB = true) {
  return story(buildLineage({
    generations: [decodeGenerationStatus(aStatus), ...(withB ? [decodeGenerationStatus(bStatus)] : []), ...extra].map((status) => ({ status })),
    revisions: [decodeRevisionObservation(revision)],
  }), A, B);
}

describe("PR149: story claims require matching evidence", () => {
  it("does not infer a provider request from a Prepared status", () => {
    const status = decodeGenerationStatus({ ...aStatus, phase: "prepared", package: null, usage: null, transport: null, request_sha256: null, wire_sha256: null });
    const first = story(buildLineage({ generations: [{ status }] }), A, B)[0]!;
    expect(first.text).not.toContain("asked the model");
  });

  it("does not infer feedback use from a parent/child observation alone", () => {
    expect(revisionStory()[2]!.text).not.toContain("from its feedback");
  });

  it("does not charge unrelated B operations to the selected revision", () => {
    const unrelated = decodeGenerationStatus({ ...bStatus, operation_id: "unrelated-b-work", phase: "failed", package: null,
      usage: { model_calls: { kind: "measured", value: 99 } } });
    expect(revisionStory([unrelated])[2]!.text).toBe(revisionStory()[2]!.text);
  });

  it("does not invent zero generation cost when the revision's status is missing", () => {
    expect(revisionStory([], false)[2]!.text).not.toContain("(0 model calls");
  });

  it("does not label a candidate as a control because generation provenance is absent", () => {
    const html = renderToStaticMarkup(<ResultsBoard lineage={scored()} orgA={A} orgB={B} />);
    expect(html).not.toContain("control: generated-method");
  });
});
