// Eric's Claude: the results matrix over every committed evidence folder,
// counting successes from the hash-named scoring metrics artifacts.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { sha256Hex } from "./digest";
import { intake } from "./intake";
import { buildLineage } from "./lineage";
import { buildMatrix, cellText, runText, type RunResult } from "./matrix";
import { decodeExecutionReceipt, decodeMetricsArtifact } from "./records";

const root = fileURLToPath(new URL("../../../../docs/invention-lab/evidence/", import.meta.url));
type F = { name: string; text: string; bytes: Uint8Array };
function evidenceFiles(): F[] {
  const out: F[] = [];
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
const file = (name: string, text: string): F => ({ name, text, bytes: new TextEncoder().encode(text) });
const runsOf = (files: F[], method: string, family: string) => {
  const l = buildLineage(intake(files).records);
  return { l, runs: buildMatrix(l).rows.find((r) => r.method.ref.id === method)?.cells.get(family) ?? [] };
};
const labels = (runs: RunResult[]) => runs.map(runText).sort();

describe("results matrix on the committed evidence", () => {
  const l = buildLineage(intake(evidenceFiles()).records);
  const m = buildMatrix(l);
  const row = (id: string) => m.rows.find((r) => r.method.ref.id === id)!;
  const cell = (id: string, f: string) => labels(row(id).cells.get(f) ?? []);

  it("reads every record without problems and finds the three families", () => {
    expect(l.problems).toEqual([]);
    expect([...m.families].sort()).toEqual(["ravine-crossing", "return-mix", "rough-transport"]);
  });

  it("reproduces the retained results run by run, keeping hidden panels and ineligible runs apart", () => {
    expect(cell("generated-method", "ravine-crossing")).toEqual(["1/12 hidden", "1/12 hidden (ineligible)", "1/6", "1/6", "4/12"]);
    expect(cell("generated-method", "rough-transport")).toEqual(["12/12", "12/12 hidden", "12/12 hidden (ineligible)", "6/6"]);
    expect(cell("generated-method", "return-mix")).toEqual(["1/6", "1/6"]);
    expect(cell("receiver-revised-method", "rough-transport")).toEqual(["3/6", "3/6", "3/6"]);
    expect(cell("receiver-revised-method", "return-mix")).toEqual(["0/6", "0/6"]);
    expect(cell("random-search", "ravine-crossing")).toEqual(["10/12", "10/12 hidden", "2/6", "2/6", "7/12 hidden (ineligible)"]);
    expect(cell("random-search", "rough-transport")).toEqual(["4/6", "5/12 hidden (ineligible)", "7/12", "9/12 hidden"]);
    expect(cell("random-search", "return-mix")).toEqual(["3/6", "3/6"]);
    expect(cell("fixed-cart", "ravine-crossing")).toEqual(["0/6"]);
    expect(cell("fixed-cart", "rough-transport")).toEqual(["2/6"]);
  });

  it("keeps each run's exact trial, role and verdict", () => {
    const rough = row("generated-method").cells.get("rough-transport")!;
    expect(rough.map((r) => [r.trial, r.role, r.org, r.verdict?.verdict ?? null, r.panel]).sort()).toEqual([
      ["hidden-st-rough-transport-comparison @ 89167965", "arm candidate", "org_example_b", null, "hidden"],
      ["hidden-st2-rough-transport-comparison @ c101e9c2", "arm candidate", "org_example_b", null, "hidden"],
      ["receive-st-rough-transport @ c3207684", "candidate", "org_example_b", "accepted", "development"],
      ["rough-transport-child-vs-parent @ 4a9eb489", "control", "org_example_b", "rejected", "development"],
    ]);
  });

  it("names a run by its own plan's hash, not another trial's with the same id", () => {
    const ravine = row("random-search").cells.get("ravine-crossing")!;
    const reference = ravine.find((r) => r.plan?.sha256.startsWith("e5e95a4c"))!;
    expect(reference.trial).toBe("example-ravine-crossing-acceptance @ e5e95a4c");
    expect(reference.verdict).toBeNull();
    const accepted = row("generated-method").cells.get("ravine-crossing")!.find((r) => r.verdict?.verdict === "accepted")!;
    expect(accepted.trial).toBe("example-ravine-crossing-acceptance @ 5e111f9a");
  });

  it("marks the runs whose usage was not measured as ineligible", () => {
    const hidden1 = row("generated-method").cells.get("ravine-crossing")!.filter((r) => r.trial.startsWith("hidden-st-"));
    expect(hidden1.map((r) => r.ineligible)).toEqual(["usage not measured: model_calls, input_tokens, output_tokens"]);
  });
});

describe("cells without a count", () => {
  const base: RunResult = {
    trial: "t @ 00000000", plan: null, role: "candidate", org: "o", successes: null, of: null,
    panel: "development", ineligible: null, verdict: null, score: { kind: "unscored" },
  };
  it("say why there is no count, never zero", () => {
    expect(runText(base)).toBe("not scored");
    expect(runText({ ...base, score: { kind: "metrics_missing" } })).toBe("metrics not supplied");
    expect(runText({ ...base, score: { kind: "not_scored", outcomes: ["unknown"] } })).toBe("scoring unknown");
    expect(runText({ ...base, score: { kind: "not_scored", outcomes: ["invalid"] } })).toBe("output invalid");
    expect(runText({ ...base, score: { kind: "rejected", reasons: ["x"] } })).toBe("metrics rejected");
    expect(runText({ ...base, score: { kind: "conflict", counts: ["1/6", "6/6"] } })).toBe("conflicting scores");
    expect(cellText(undefined)).toBe("–");
  });

  it("drops a metrics artifact whose bytes do not match the evaluation's hash", () => {
    const files = evidenceFiles().map((f) =>
      f.name.endsWith("candidate-score/artifacts/db11d4ff7ed356ee1ff084f185a0e7483fab71d1147892d38d2abc59d19ddc60")
        ? file(f.name, f.text.replace('"success":false', '"success":true'))
        : f,
    );
    const { runs } = runsOf(files, "generated-method", "ravine-crossing");
    expect(runs.find((r) => r.trial.endsWith("@ 5e111f9a"))!.score.kind).toBe("metrics_missing");
  });

  it("keeps a byte-exact metrics file even when a reformatted copy comes first", () => {
    const target = "live-method-20261024-receiver/candidate-score/artifacts/db11d4ff7ed356ee1ff084f185a0e7483fab71d1147892d38d2abc59d19ddc60";
    const files = evidenceFiles();
    const pretty = JSON.stringify(JSON.parse(files.find((f) => f.name === target)!.text), null, 2);
    const { runs } = runsOf([file("pretty.json", pretty), ...files], "generated-method", "ravine-crossing");
    expect(runText(runs.find((r) => r.trial.endsWith("@ 5e111f9a"))!)).toBe("1/6");
  });

  it("binds metrics by the file's own bytes, not by re-encoding its decoded text", () => {
    const target = "live-method-20261024-receiver/candidate-score/artifacts/db11d4ff7ed356ee1ff084f185a0e7483fab71d1147892d38d2abc59d19ddc60";
    const ev = "live-method-20261024-receiver/candidate-score/evaluation.json";
    const files = evidenceFiles();
    const orig = files.find((f) => f.name === target)!;
    // A UTF-8 byte-order mark is dropped by decoding, so the text alone hashes differently.
    const bytes = new Uint8Array([0xef, 0xbb, 0xbf, ...orig.bytes]);
    const sha = sha256Hex(bytes);
    const evJson = JSON.parse(files.find((f) => f.name === ev)!.text);
    evJson.outcome.metrics.sha256 = sha;
    const edited = files
      .filter((f) => f !== orig)
      .map((f) => (f.name === ev ? file(ev, JSON.stringify(evJson)) : f))
      .concat({ name: `bom/artifacts/${sha}`, text: new TextDecoder().decode(bytes), bytes });
    const { runs } = runsOf(edited, "generated-method", "ravine-crossing");
    expect(runText(runs.find((r) => r.trial.endsWith("@ 5e111f9a"))!)).toBe("1/6");
  });
});

// B's revision against the fixed cart on rough-transport: retained 3/6, accepted.
const EVAL = "live-method-20261024-b-revision-receiver/vs-fixed-cart/candidate-score/evaluation.json";
type Row = { instance: string; success: boolean };
function editScoring(rows: (r: Row[]) => Row[], ev: (e: any) => void = () => {}, metrics: (m: any) => void = () => {}): F[] {
  const files = evidenceFiles();
  const evFile = files.find((f) => f.name === EVAL)!;
  const evJson = JSON.parse(evFile.text);
  const m = JSON.parse(files.find((f) => f.name.endsWith(`vs-fixed-cart/candidate-score/artifacts/${evJson.outcome.metrics.sha256}`))!.text);
  m.instances = rows(m.instances);
  metrics(m);
  const text = JSON.stringify(m);
  const sha = sha256Hex(new TextEncoder().encode(text));
  evJson.outcome.metrics.sha256 = sha;
  ev(evJson);
  return files.map((f) => (f === evFile ? file(EVAL, JSON.stringify(evJson)) : f)).concat(file(`edit/artifacts/${sha}`, text));
}
const fixedCartRun = (files: F[]) => {
  const { l, runs } = runsOf(files, "receiver-revised-method", "rough-transport");
  return { problems: l.problems, run: runs.find((r) => r.trial.startsWith("example-rough-transport-acceptance"))! };
};

describe("a scored evaluation is counted only when it is consistent", () => {
  it("counts the unedited run", () => {
    const { run, problems } = fixedCartRun(evidenceFiles());
    expect([runText(run), problems]).toEqual(["3/6", []]);
  });

  it("refuses repeated or missing instance rows and says why", () => {
    const dup = fixedCartRun(editScoring((rows) => rows.map((r, i) => (i === rows.findIndex((x) => !x.success) ? { ...rows.find((x) => x.success)! } : r))));
    expect(runText(dup.run)).toBe("metrics rejected");
    expect(dup.problems.join("\n")).toContain("repeat an instance row");
    const missing = fixedCartRun(editScoring((rows) => rows.slice(1), () => {}, (m) => { m.of = 5; m.success_count = 3; }));
    expect(runText(missing.run)).toBe("metrics rejected");
    expect(missing.problems.join("\n")).toContain("not the plan's development instances");
  });

  it("refuses totals that disagree with the rows", () => {
    const { run, problems } = fixedCartRun(editScoring((rows) => rows, () => {}, (m) => { m.success_count = 4; }));
    expect(runText(run)).toBe("metrics rejected");
    expect(problems.join("\n")).toContain("success_count = 4");
  });

  it("refuses an evaluation scored against another task, or whose observed context differs", () => {
    const ravine = { domain: "genesis-lab", schema: "lab-task/1", id: "ravine-crossing", version: "1", sha256: "1".repeat(64) };
    const other = fixedCartRun(editScoring((rows) => rows, (e) => { e.request.context.public_task = ravine; e.observed_context.public_task = ravine; }));
    expect(runText(other.run)).toBe("metrics rejected");
    expect(other.problems.join("\n")).toContain("scored against task ravine-crossing");
    const drift = fixedCartRun(editScoring((rows) => rows, (e) => { e.observed_context.scoring_rule.sha256 = "0".repeat(64); }));
    expect(runText(drift.run)).toBe("metrics rejected");
    expect(drift.problems.join("\n")).toContain("observed context differs");
  });

  it("does not count a metrics reference with an unsupported identity", () => {
    const { run } = fixedCartRun(editScoring((rows) => rows, (e) => { e.outcome.metrics.schema = "foreign-metric/99"; }));
    expect(run.successes).toBeNull();
    expect(runText(run)).toBe("metrics rejected");
  });

  it("reads an unknown scoring outcome as unknown, not as missing metrics", () => {
    const { run } = fixedCartRun(editScoring((rows) => rows, (e) => { e.outcome = { kind: "unknown", reason: "interrupted" }; }));
    expect(runText(run)).toBe("scoring unknown");
  });

  it("shows a conflict, whatever the file order, when two scorings of one run disagree", () => {
    const base = evidenceFiles();
    const ev = JSON.parse(base.find((f) => f.name === EVAL)!.text);
    const m = JSON.parse(base.find((f) => f.name.endsWith(`artifacts/${ev.outcome.metrics.sha256}`))!.text);
    for (const r of m.instances) r.success = true;
    m.success_count = m.instances.length;
    const text = JSON.stringify(m);
    ev.request.request_id = "score-candidate-again";
    ev.outcome.metrics.sha256 = sha256Hex(new TextEncoder().encode(text));
    const extra = [file("again/evaluation.json", JSON.stringify(ev)), file(`again/artifacts/${ev.outcome.metrics.sha256}`, text)];
    const first = fixedCartRun([...extra, ...base]);
    const last = fixedCartRun([...base, ...extra]);
    expect(runText(first.run)).toBe("conflicting scores");
    expect(runText(last.run)).toBe("conflicting scores");
    expect(first.run.score).toEqual(last.run.score);
    expect(first.problems.join("\n")).toContain("different results (3/6, 6/6)");
  });

  it("reports a second scoring whose metrics were not supplied instead of dropping it", () => {
    const base = evidenceFiles();
    const ev = JSON.parse(base.find((f) => f.name === EVAL)!.text);
    ev.request.request_id = "score-candidate-unchecked";
    ev.outcome.metrics.sha256 = "a".repeat(64);
    const { run, problems } = fixedCartRun([...base, file("unchecked/evaluation.json", JSON.stringify(ev))]);
    expect(runText(run)).toBe("3/6");
    expect(problems.join("\n")).toContain("score-candidate-unchecked names metrics that were not supplied");
  });

  it("keeps a run with no evaluation as not scored", () => {
    const files = evidenceFiles().filter((f) => f.name !== EVAL);
    expect(runText(fixedCartRun(files).run)).toBe("not scored");
  });
});

describe("identities", () => {
  it("splits one task id under two exact references into two columns and says so", () => {
    const rec = intake(evidenceFiles()).records;
    const e = rec.executions!.find((x) => x.public_task.id === "rough-transport")!;
    e.public_task = { ...e.public_task, version: "2", sha256: "f".repeat(64) };
    const l = buildLineage(rec);
    const m = buildMatrix(l);
    expect(m.families).toHaveLength(4);
    expect(m.families.filter((f) => f.startsWith("rough-transport@"))).toHaveLength(2);
    expect(l.problems.join("\n")).toContain("task family rough-transport appears under 2 different exact references");
    expect([...m.familyLabels.values()].filter((x) => x.startsWith("rough-transport@")).sort()).toEqual([
      "rough-transport@1 genesis-lab/lab-task/1 7ea1854c2b35",
      "rough-transport@2 genesis-lab/lab-task/1 ffffffffffff",
    ]);
  });

  it("requires the binding's task in an execution receipt", () => {
    const receipt = JSON.parse(evidenceFiles().find((f) => f.name === "live-method-20261024-receiver/candidate/receipt.json")!.text);
    delete receipt.binding.public_task;
    expect(() => decodeExecutionReceipt(receipt)).toThrow(/public_task/);
  });

  it("does not decode a metrics file with no instance rows", () => {
    expect(() => decodeMetricsArtifact(new TextEncoder().encode('{"instances":[],"of":0,"success_count":0}'))).toThrow(/non-empty/);
  });
});
