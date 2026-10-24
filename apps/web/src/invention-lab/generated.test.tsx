// Eric's Claude: the first live generated method package (#95), read from the
// committed evidence. The package file's bytes are reconstructed from its JSON
// import; the first assertion proves they hash to the recorded identity.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { beats } from "./beats";
import { intake } from "./intake";
import { buildLedger, totalText } from "./ledger";
import { buildLineage } from "./lineage";
import { LineagePanel } from "./LineagePanel";
import s1 from "../../../../docs/invention-lab/evidence/live-method-20261024/final-status.json";
import g1 from "../../../../docs/invention-lab/evidence/live-method-20261024/generation.json";
import s2 from "../../../../docs/invention-lab/evidence/live-method-20261024-diagnostic/final-status.json";
import g2 from "../../../../docs/invention-lab/evidence/live-method-20261024-diagnostic/generation.json";
import s3 from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/final-status.json";
import g3 from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/generation.json";
import p3 from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/prepared-generation.json";
import pkg from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/method-package.json";
import inspection from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/static-inspection.json";

const PACKAGE_SHA = "8215c3679a06724069759bcbdaa576b64ec0ef1251c5238a797c0cb34962b944";
const SOURCE_SHA = "68e990eaebff8a4c69e152aa98743a1f50fd318d031f57b83b78170707b1ab31";
const file = (name: string, v: unknown) => ({ name, text: JSON.stringify(v) });
const textOf = (html: string) => html.replace(/<[^>]+>/g, " ").replace(/&#x27;/g, "'").replace(/\s+/g, " ");
const all = () =>
  intake([
    file("1/final-status.json", s1),
    file("1/generation.json", g1),
    file("2/final-status.json", s2),
    file("2/generation.json", g2),
    file("3/final-status.json", s3),
    file("3/generation.json", g3),
    file("3/prepared-generation.json", p3),
    file("3/method-package.json", pkg),
    file("3/static-inspection.json", inspection),
  ]);

describe("first generated package", () => {
  it("hashes the package file to the identity its generation status recorded", async () => {
    const i = await all();
    const p = i.records.packages![0]!;
    expect(p.ref).toEqual({ domain: "genesis", schema: "development-method-source/1", id: "generated-method", version: "0.1.0", sha256: PACKAGE_SHA });
    expect(p.files).toEqual([{ path: "method.py", bytes: 4027, lines: 137, sha256: SOURCE_SHA }]);
    expect(p.runtime.sha256).toBe("0b21e795ac528c0f9cbe99e28ca1125af0c4dc5b87f53224abfe2a2ae5a69491");
    expect(p.protocol.sha256).toBe("d3b90c36b32e89b4e3d191dc018717301a4afc386ac915123cf5cf86250c550d");
  });

  it("files the package and its inspection under one generated node with no problems", async () => {
    const l = buildLineage((await all()).records);
    expect(l.problems).toEqual([]);
    expect(l.nodes).toHaveLength(1);
    const n = l.nodes[0]!;
    expect(n.package?.ref.sha256).toBe(PACKAGE_SHA);
    expect(n.inspections.map((x) => [x.parse, x.imports.join(","), x.execution])).toEqual([["python_ast_parse: passed", "json,math,sys", "execution: not_performed"]]);
    expect(l.edges).toEqual([{ kind: "generated", org_id: "org_example_a", operation_id: "live-a-method-20261024t160859z-75cdf274", to: n.key }]);
  });

  it("totals all three attempts as the #95 README does", async () => {
    const l = buildLedger(buildLineage((await all()).records).attempts);
    expect([l.attempts, l.packages, l.byPhase]).toEqual([3, 1, { failed: 2, generated: 1 }]);
    expect(["model_calls", "input_tokens", "output_tokens", "wall_time_ms"].map((d) => totalText(l.totals[d as "model_calls"]))).toEqual([
      "3",
      "12027",
      "18315",
      "141119",
    ]);
  });

  it("records A's first package while every later beat still has no record", async () => {
    const b = beats(buildLineage((await all()).records), "org_example_a", "org_example_b");
    expect(b[0]!.facts).toEqual([
      { text: "org_example_a generation attempts: 3, 1 with a package", recorded: true },
      { text: "org_example_a runs: none recorded", recorded: false },
    ]);
    expect(b.slice(1).flatMap((x) => x.facts).every((f) => !f.recorded)).toBe(true);
    expect(b[4]!.facts[0]!.text).toBe("chain incomplete; no record of a revision by org_example_b, org_example_a's decision on that revision");
  });

  it("renders the version with its byte check and the inspection's own limit", async () => {
    const text = textOf(renderToStaticMarkup(<LineagePanel lineage={buildLineage((await all()).records)} orgA="org_example_a" orgB="org_example_b" />));
    expect(text).toContain("generated-method@0.1.0 8215c3679a06");
    expect(text).toContain("package file: its bytes hash to exactly this version (checked in this browser)");
    expect(text).toContain("method.py 4027 bytes / 137 lines");
    expect(text).toContain("python_ast_parse: passed; imports json, math, sys; execution: not_performed.");
    expect(text).toContain("syntax and observed imports only; not runtime, safety, acceptance or improvement evidence");
    expect(text).toContain("generated by org_example_a in live-a-method-20261024t160859z-75cdf274");
    expect(text).toContain("3 attempts · 2 generation failed · 1 generated source · 1 package");
  });
});

describe("package and inspection mismatches", () => {
  it("report a package file whose bytes differ from the recorded version", async () => {
    const tampered = { ...pkg, package: { ...pkg.package, files: [{ ...pkg.package.files[0]!, content: "print(1)\n" }] } };
    const i = await intake([file("s.json", s3), file("m.json", tampered)]);
    const l = buildLineage(i.records);
    expect(l.problems).toHaveLength(1);
    expect(l.problems[0]).toMatch(/^package file for generated-method@0\.1\.0 hashes to [0-9a-f]{12}, not the recorded 8215c3679a06$/);
    expect(l.nodes).toHaveLength(2);
  });

  it("report an inspection of other source bytes, and one naming no supplied package", async () => {
    const other = { ...inspection, source_sha256: "0".repeat(64) };
    const l = buildLineage((await intake([file("s.json", s3), file("m.json", pkg), file("i.json", other)])).records);
    expect(l.problems).toEqual(["static inspection of method.py names different source bytes than the supplied package"]);
    const orphan = buildLineage((await intake([file("i.json", inspection)])).records);
    expect(orphan.problems).toEqual(["static inspection names package 8215c3679a06, which no supplied record contains"]);
  });
});
