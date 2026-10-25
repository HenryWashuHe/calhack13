// Independent UI-only regressions: retained JSON is parsed, never executed.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { intake } from "./intake";
import { buildLineage } from "./lineage";
import aPackage from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/method-package.json";
import bPackage from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/method-package.json";
import bInspection from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/static-inspection.json";
import plan from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/plan.json";
import verdict from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/verdict.json";

const file = (name: string, value: unknown) => ({ name, text: JSON.stringify(value) });

describe("independent real-evidence intake boundaries", () => {
  it("retains equal verdict values separately when their trial folders differ", () => {
    const otherPlan = { ...structuredClone(plan), id: "independent-second-trial" };
    const i = intake([
      file("first/plan.json", plan), file("first/verdict.json", verdict),
      file("second/plan.json", otherPlan), file("second/verdict.json", verdict),
    ]);
    expect(i.records.trials).toHaveLength(2);
    expect(i.records.trials!.map((t) => t.verdict?.verdict)).toEqual(["accepted", "accepted"]);
    expect(i.used.filter((n) => n.file.endsWith("verdict.json"))).toHaveLength(2);
  });

  it("does not discard distinct raw package identities as canonical JSON duplicates", () => {
    const canonical = file("original.json", aPackage);
    const formatted = { name: "formatted.json", text: JSON.stringify(aPackage, null, 2) };
    const i = intake([formatted, canonical]);
    expect(i.records.packages).toHaveLength(2);
    expect(new Set(i.records.packages!.map((p) => p.ref.sha256)).size).toBe(2);
    const expected = intake([canonical]).records.packages![0]!.ref;
    expect(i.records.packages!.some((p) => p.ref.sha256 === expected.sha256)).toBe(true);
    expect(buildLineage(i.records).problems).not.toEqual([]);
  });

  it("does not silently attach an inspection whose explicit exact reference contradicts its package", () => {
    const foreign = { ...structuredClone(bInspection), package: { ...bInspection.package, id: "foreign-package" } };
    const l = buildLineage(intake([file("method.json", bPackage), file("inspection.json", foreign)]).records);
    const inspected = l.nodes.find((n) => n.package)!;
    // Either refuse attribution or retain a visible contradiction; never silently
    // turn an explicit foreign reference into proof about the supplied package.
    expect(inspected.inspections.length === 0 || l.problems.length > 0).toBe(true);
  });

  it("deduplicates exact package copies and keeps the real B inspection without inventing execution", () => {
    const i = intake([file("a/method.json", bPackage), file("b/copy.json", bPackage), file("b/inspection.json", bInspection)]);
    expect(i.records.packages).toHaveLength(1);
    const l = buildLineage(i.records);
    expect(l.problems).toEqual([]);
    expect(l.nodes[0]!.inspections).toHaveLength(1);
    expect(l.nodes[0]!.inspections[0]!.execution).toBe("source_executed false");
    expect(l.executions).toEqual([]);
  });

  it("reads the committed receiver, B-revision receiver and A-return trials without claiming adoption", () => {
    const root = fileURLToPath(new URL("../../../../docs/invention-lab/evidence/", import.meta.url));
    const records: { name: string; text: string }[] = [];
    const walk = (dir: string) => {
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        const p = path.join(dir, entry.name);
        if (entry.isDirectory()) walk(p);
        else if (["plan.json", "verdict.json", "receipt.json", "evaluation.json"].includes(entry.name))
          records.push({ name: path.relative(root, p), text: fs.readFileSync(p, "utf8") });
      }
    };
    for (const folder of ["live-method-20261024-receiver", "live-method-20261024-b-revision-receiver", "live-method-20261024-a-return"])
      walk(path.join(root, folder));
    const l = buildLineage(intake(records).records);
    expect(l.problems).toEqual([]);
    expect(l.trials).toHaveLength(9);
    expect(l.trials.filter((t) => t.verdict)).toHaveLength(8);
    expect(l.trials.filter((t) => t.verdict?.verdict === "accepted")).toHaveLength(2);
    expect(l.trials.filter((t) => t.verdict?.verdict === "rejected")).toHaveLength(6);
    expect(l.executions).toHaveLength(17);
    expect(l.edges.some((e) => e.kind === "adoption")).toBe(false);
  });
});
