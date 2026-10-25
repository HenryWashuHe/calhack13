// Read retained JSON only. No archived source, executor, scorer or provider runs.
import fs from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";
import { intake } from "./intake";
import { purposeText } from "./claims";
import { buildLineage } from "./lineage";
import { buildMatrix } from "./matrix";
import { firstPassEvidenceFiles, FIRST_PASS_FOLDERS } from "./test-fixtures/first-pass";

it("keeps a later rehearsal visible and unsupported without widening the first-pass fixture", () => {
  const firstPass = firstPassEvidenceFiles();
  expect([...new Set(firstPass.map((f) => f.name.split("/")[0]))]).toEqual([...FIRST_PASS_FOLDERS]);
  expect(firstPass.some((f) => f.name.startsWith("rehearsal-"))).toBe(false);

  const folder = "rehearsal-two-orgs-20261025/orgs/run/";
  const files = ["state/receipt.json", "plan.json", "score/evaluation.json"].map((file) => {
    const name = folder + file;
    const bytes = new Uint8Array(fs.readFileSync(fileURLToPath(new URL(`../../../../docs/invention-lab/evidence/${name}`, import.meta.url))));
    return { name, bytes, text: new TextDecoder().decode(bytes) };
  });
  const i = intake(files);
  expect(i.used.map((u) => u.file).sort()).toEqual(files.map((f) => f.name).sort());
  expect(i.skipped).toEqual([]);
  const l = buildLineage(i.records);
  const execution = l.executions.find((e) => e.receipt.run_id === "run-invoke-rehearsal3-20261025")!;
  expect(execution.receipt.purpose).toEqual({ kind: "unrecognized", raw: "invocation" });
  expect(purposeText(execution.receipt.purpose)).toBe("unrecognized purpose: invocation");
  expect(execution.evaluations).toEqual([]);
  expect(execution.score.kind).toBe("unscored");
  expect(l.trials.every((t) => !t.executions.includes(execution))).toBe(true);
  expect(l.problems).toContain("evaluation score-invoke-rehearsal3-20261025 has a different purpose than its named execution; not attached");

  const result = buildMatrix(l).rows.flatMap((r) => [...r.cells.values()].flat()).find((r) => r.role === "invocation")!;
  expect(result).toMatchObject({ successes: null, of: null, panel: "unknown", verdict: null });
});
