// Eric's Claude: the results matrix over every committed evidence folder,
// counting successes from the hash-named scoring metrics artifacts.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { intake } from "./intake";
import { buildLineage } from "./lineage";
import { buildMatrix, cellText } from "./matrix";

const root = fileURLToPath(new URL("../../../../docs/invention-lab/evidence/", import.meta.url));
function evidenceFiles() {
  const out: { name: string; text: string; bytes: Uint8Array }[] = [];
  const walk = (dir: string) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (!/\.(md|txt)$|SHA256SUMS$/.test(e.name)) {
        const bytes = new Uint8Array(fs.readFileSync(p));
        out.push({ name: path.relative(root, p), text: new TextDecoder().decode(bytes), bytes });
      }
    }
  };
  walk(root);
  return out;
}

describe("results matrix on the committed evidence", () => {
  const l = buildLineage(intake(evidenceFiles()).records);
  const m = buildMatrix(l);
  const row = (id: string) => m.rows.find((r) => r.method.ref.id === id)!;
  const cells = (id: string) => ["ravine-crossing", "rough-transport", "return-mix"].map((f) => cellText(row(id).cells.get(f)));

  it("reads every record without problems and finds the three families", () => {
    expect(l.problems).toEqual([]);
    expect([...m.families].sort()).toEqual(["ravine-crossing", "return-mix", "rough-transport"]);
  });

  it("reproduces the retained scoreboard from metrics artifacts, run by run", () => {
    expect(cells("generated-method")).toEqual(["1/6", "6/6", "1/6"]);
    expect(cells("receiver-revised-method")).toEqual(["–", "3/6", "0/6"]);
    expect(cells("random-search")).toEqual(["2/6", "4/6", "3/6"]);
    expect(cells("fixed-cart")).toEqual(["0/6", "2/6", "–"]);
  });

  it("keeps each run's trial, role and verdict", () => {
    const rough = row("generated-method").cells.get("rough-transport")!;
    expect(rough.map((r) => [r.trial, r.role, r.org, r.verdict?.verdict])).toEqual([
      ["rough-transport-child-vs-parent", "control", "org_example_b", "rejected"],
    ]);
  });
});

describe("cells without metrics", () => {
  it("say whether the run was scored, never zero", () => {
    expect(cellText([{ trial: "t", role: "candidate", org: "o", scored: true, successes: null, of: null, verdict: null }])).toBe("metrics not supplied");
    expect(cellText([{ trial: "t", role: "candidate", org: "o", scored: false, successes: null, of: null, verdict: null }])).toBe("not scored");
    expect(cellText(undefined)).toBe("–");
  });

  it("drops a metrics artifact whose bytes do not match the evaluation's hash", () => {
    const files = evidenceFiles().map((f) =>
      f.name.endsWith("candidate-score/artifacts/db11d4ff7ed356ee1ff084f185a0e7483fab71d1147892d38d2abc59d19ddc60")
        ? { ...f, bytes: new TextEncoder().encode(f.text.replace('"success":false', '"success":true')), text: f.text.replace('"success":false', '"success":true') }
        : f,
    );
    const m = buildMatrix(buildLineage(intake(files).records));
    const ravine = m.rows.find((r) => r.method.ref.id === "generated-method")!.cells.get("ravine-crossing")!;
    expect(ravine.map((r) => (r.successes === null ? "none" : r.successes))).toContain("none");
  });

  it("keeps a byte-exact metrics file even when a reformatted copy comes first", () => {
    const target = "candidate-score/artifacts/db11d4ff7ed356ee1ff084f185a0e7483fab71d1147892d38d2abc59d19ddc60";
    const files = evidenceFiles();
    const exact = files.find((f) => f.name.endsWith(`live-method-20261024-receiver/${target}`))!;
    const pretty = JSON.stringify(JSON.parse(exact.text), null, 2);
    const m = buildMatrix(buildLineage(intake([{ name: "pretty.json", text: pretty, bytes: new TextEncoder().encode(pretty) }, ...files]).records));
    const ravine = m.rows.find((r) => r.method.ref.id === "generated-method")!.cells.get("ravine-crossing")!;
    expect(cellText(ravine)).toBe("1/6");
  });
});
