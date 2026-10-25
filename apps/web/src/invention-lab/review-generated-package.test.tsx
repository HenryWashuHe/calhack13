// Independent PR109 guards: retained JSON and React rendering only.
// Package content is never evaluated, imported as code, or executed.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import pkg from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/method-package.json";
import status from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/final-status.json";
import inspection from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/static-inspection.json";
import { intake } from "./intake";
import { buildLineage } from "./lineage";
import { LineagePanel } from "./LineagePanel";

const file = (name: string, value: unknown) => {
  const text = JSON.stringify(value);
  return { name, text, bytes: new TextEncoder().encode(text) };
};

describe("PR109 package format and inspection attribution", () => {
  it.each([
    { label: "missing version", version: undefined },
    { label: "numeric version 2", version: 2 },
    { label: "string version 1", version: "1" },
  ])("does not label $label as a checked v1 package", async ({ version }) => {
    const result = await intake([file("unsupported-package.json", { ...pkg, version })]);
    expect(result.records.packages).toHaveLength(0);
    expect(result.skipped).toHaveLength(1);
    expect(result.used).toHaveLength(0);
  });

  it("does not attach a matching inspection to a contradictory reference sharing only the hash", async () => {
    // An untrusted status can copy a real hash under a different identity. The
    // actual supplied bytes identify generated-method, not the foreign card.
    const foreign = {
      ...status,
      operation_id: "foreign-reference",
      package: { ...status.package, id: "not-the-package-id" },
    };
    const result = await intake([
      file("foreign-status.json", foreign),
      file("method-package.json", pkg),
      file("inspection.json", inspection),
    ]);
    const lineage = buildLineage(result.records);
    const wrong = lineage.nodes.find((n) => n.ref.id === "not-the-package-id")!;
    const exact = lineage.nodes.find((n) => n.package)!;
    expect(exact.ref.id).toBe(pkg.package.id);
    expect(wrong.inspections).toHaveLength(0);
    // It may attach to the exact byte-derived card or explicitly refuse an
    // ambiguous identity; it must not silently attach to the contradictory one.
    expect(exact.inspections.length === 1 || lineage.problems.length > 0).toBe(true);
  });

  it("keeps the real package and matching inspection together without an execution claim", async () => {
    const hostileClaim = "<script>globalThis.reviewCanary = true</script>";
    const result = await intake([
      file("status.json", status),
      file("method-package.json", pkg),
      file("inspection.json", { ...inspection, claim: hostileClaim }),
    ]);
    const lineage = buildLineage(result.records);
    expect(lineage.problems).toEqual([]);
    expect(lineage.nodes).toHaveLength(1);
    expect(lineage.nodes[0]!.package!.ref.sha256).toBe(inspection.method_package_sha256);
    expect(lineage.nodes[0]!.inspections).toHaveLength(1);
    expect(lineage.executions).toEqual([]);
    const html = renderToStaticMarkup(<LineagePanel lineage={lineage} />);
    expect(html).toContain("execution not_performed");
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
  });
});
