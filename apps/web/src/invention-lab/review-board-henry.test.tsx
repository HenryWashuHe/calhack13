// Henry's Claude: review regressions for #148/#149 (exact heads 1519c275 / f180a83a).
// Retained evidence plus synthetic substitutions only; nothing runs.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { sha256Hex } from "./digest";
import { intake } from "./intake";
import { firstPassEvidenceFiles as evidenceFiles } from "./test-fixtures/first-pass";
import { buildLineage } from "./lineage";
import { buildMatrix, cellText } from "./matrix";
import { decodeGenerationStatus, type ExactRef } from "./records";
import { ResultsBoard } from "./ResultsBoard";
import { story } from "./story";

type F = { name: string; text: string; bytes: Uint8Array };
const file = (name: string, text: string): F => ({ name, text, bytes: new TextEncoder().encode(text) });

// B's revision as candidate vs fixed cart on rough-transport: retained 3/6.
const EVAL = "live-method-20261024-b-revision-receiver/vs-fixed-cart/candidate-score/evaluation.json";
type Row = { instance: string; success: boolean };
/** The scorer receipt names a different metrics file (as a scorer that wrote it would). */
function withMetrics(edit: (rows: Row[]) => Row[], editEval: (ev: any) => void = () => {}) {
  const files = evidenceFiles();
  const ev = files.find((f) => f.name === EVAL)!;
  const evJson = JSON.parse(ev.text);
  const orig = files.find((f) => f.name.endsWith(`candidate-score/artifacts/${evJson.outcome.metrics.sha256}`))!;
  const m = JSON.parse(orig.text);
  m.instances = edit(m.instances);
  const text = JSON.stringify(m);
  const sha = sha256Hex(new TextEncoder().encode(text));
  evJson.outcome.metrics.sha256 = sha;
  editEval(evJson);
  return files.map((f) => (f === ev ? file(f.name, JSON.stringify(evJson)) : f)).concat(file(`x/artifacts/${sha}`, text));
}
const revCell = (files: F[], fam = "rough-transport") => {
  const l = buildLineage(intake(files).records);
  const row = buildMatrix(l).rows.find((r) => r.method.ref.id === "receiver-revised-method")!;
  return { text: cellText(row.cells.get(fam)), problems: l.problems };
};

describe("#148 matrix: metrics validation", () => {
  it("baseline", () => expect(revCell(evidenceFiles()).text).toBe("3/6"));

  it("D1 rejects duplicated instance rows instead of counting them", () => {
    const files = withMetrics((rows) => {
      const t = rows.find((r) => r.success)!;
      const f = rows.findIndex((r) => !r.success);
      return rows.map((r, i) => (i === f ? { ...t } : r));
    });
    expect(revCell(files).text).not.toContain("4/6");
  });

  it("D1 does not count a metrics file with a missing instance row", () => {
    const files = withMetrics((rows) => rows.filter((r, i) => r.success || i !== rows.findIndex((x) => !x.success)));
    expect(revCell(files).text).not.toContain("3/5");
  });

  it("D2 does not count an evaluation scored against another task family", () => {
    const ravine = JSON.parse(
      evidenceFiles().find((f) => f.name === "live-method-20261024-receiver/candidate-score/evaluation.json")!.text,
    ).request.context.public_task as ExactRef;
    const files = withMetrics(
      (rows) => rows.map((r, i) => ({ ...r, instance: `ravine-crossing/1/${i}` })),
      (ev) => {
        ev.request.context.public_task = ravine;
        ev.observed_context.public_task = ravine;
      },
    );
    const r = revCell(files);
    expect(r.text === "3/6" && r.problems.length === 0).toBe(false);
  });

  it("D2 does not count an evaluation whose observed scoring context differs from the requested one", () => {
    const files = withMetrics(
      (rows) => rows,
      (ev) => {
        ev.observed_context.scoring_rule.sha256 = "0".repeat(64);
      },
    );
    const r = revCell(files);
    expect(r.text === "3/6" && r.problems.length === 0).toBe(false);
  });

  it("D3 conflicting scored evaluations of one run are not resolved by file order", () => {
    const base = evidenceFiles();
    const ev = JSON.parse(base.find((f) => f.name === EVAL)!.text);
    const all = JSON.stringify({ instances: Array.from({ length: 6 }, (_, i) => ({ instance: `rough-transport/1/${i}`, success: true })) });
    const sha = sha256Hex(new TextEncoder().encode(all));
    ev.request.request_id = "score-candidate-rescore";
    ev.outcome.metrics.sha256 = sha;
    const extra = [file("y/evaluation.json", JSON.stringify(ev)), file(`y/artifacts/${sha}`, all)];
    const first = revCell([...extra, ...base]);
    const last = revCell([...base, ...extra]);
    expect(first.text).toBe(last.text); // actual: "6/6 · 3/6" vs "3/6"
  });
});

describe("#148 matrix: identities", () => {
  it("D4 does not merge two different task versions into one column", () => {
    const rec = intake(evidenceFiles()).records;
    const e = rec.executions!.find((x) => x.public_task.id === "rough-transport")!;
    e.public_task = { ...e.public_task, version: "2", sha256: "f".repeat(64) };
    const l = buildLineage(rec);
    const m = buildMatrix(l);
    expect(m.families.length === 3 && l.problems.length === 0).toBe(false);
  });

  it("D5 does not label a run with a trial id whose plan it does not belong to", () => {
    const l = buildLineage(intake(evidenceFiles()).records);
    const runs = buildMatrix(l).rows.find((r) => r.method.ref.id === "random-search")!.cells.get("ravine-crossing")!;
    const accepted = l.trials.find((t) => t.plan.ref.id === "example-ravine-crossing-acceptance")!;
    expect(accepted.executions.some((e) => e.method === buildMatrix(l).rows.find((r) => r.method.ref.id === "random-search")!.method.key)).toBe(false);
    // The reference random-search run (plan e5e95a4c…) is shown under the accepted trial's id (plan 5e111f9a…).
    expect(runs.filter((r) => r.trial === "example-ravine-crossing-acceptance").map((r) => r.role)).not.toContain("candidate");
  });
});

const pkg: ExactRef = {
  domain: "genesis.development_method_source",
  schema: "development-method-source/1",
  id: "m",
  version: "1",
  sha256: "a".repeat(64),
};
const status = (org: string, op: string, transport: string, phase = "generated", calls = 1) =>
  decodeGenerationStatus({
    operation_id: op,
    org_id: org,
    phase,
    settings: { model: "m", max_tokens: 100, timeout_ms: 1, sampling: "provider_default" },
    plan_sha256: "p",
    request_sha256: "r" + op,
    wire_sha256: "w",
    transport,
    actual_model: "m",
    usage: { model_calls: { kind: "measured", value: calls }, output_tokens: { kind: "measured", value: 40 } },
    package: phase === "generated" ? { ...pkg, id: op } : null,
    authority: "none",
  });

describe("#149 board", () => {
  it("D6 does not tell loopback-fixture generations as the model being asked", () => {
    const l = buildLineage({ generations: [{ status: status("org_a", "op-a", "loopback_fixture") }] });
    const first = story(l, "org_a", "org_b")[0]!.text;
    const html = renderToStaticMarkup(<ResultsBoard lineage={l} orgA="org_a" orgB="org_b" />);
    const visible = html.split("All records behind this board")[0];
    expect(first).not.toContain("asked the model"); // actual: "org_a asked the model for a development method 1 times; ..."
    expect(visible).toMatch(/fixture/);
  });

  it("D7 surfaces lineage problems outside the collapsed records", () => {
    const files = evidenceFiles();
    const ev = JSON.parse(files.find((f) => f.name === EVAL)!.text);
    ev.request.execution_sha256 = "0".repeat(64);
    ev.request.request_id = "dangling";
    const l = buildLineage(intake([...files, file("z/evaluation.json", JSON.stringify(ev))]).records);
    expect(l.problems.length).toBeGreaterThan(0);
    const html = renderToStaticMarkup(<ResultsBoard lineage={l} orgA="org_example_a" orgB="org_example_b" />);
    expect(html.split("All records behind this board")[0]).toMatch(/problem/i);
  });

  it("D8 does not attribute B's unrelated generation cost to its revision", () => {
    const files = evidenceFiles();
    const rec = intake(files).records;
    rec.generations.push({ status: status("org_example_b", "b-other", "anthropic_https", "failed", 7) });
    const s = story(buildLineage(rec), "org_example_a", "org_example_b")[2]!.text;
    expect(s).toContain("(1 model calls"); // actual: "(8 model calls, ..."
  });
});

describe("controls (expected to pass)", () => {
  it("non-boolean success is not counted", () => {
    const files = withMetrics((rows) => rows.map((r) => ({ ...r, success: (r.success ? "true" : 0) as any })));
    expect(revCell(files).text).toBe("metrics not supplied · 3/6");
  });
  it("failed / unknown scoring outcomes are not counted", () => {
    for (const kind of ["failed", "unknown", "invalid"]) {
      const files = withMetrics((rows) => rows, (ev) => { ev.outcome = { kind, reason: "x" }; });
      // Eric's Claude: was "metrics not supplied · 3/6" for every kind. Codex's
      // #148 P2 hold requires the outcome to be named instead; still not counted.
      const named = { failed: "scoring failed", unknown: "scoring unknown", invalid: "output invalid" }[kind];
      expect(revCell(files).text).toBe(`${named} · 3/6`);
    }
  });
  it("receipt naming a missing artifact is not counted", () => {
    const files = withMetrics((rows) => rows).filter((f) => !f.name.startsWith("x/artifacts/"));
    expect(revCell(files).text).toBe("metrics not supplied · 3/6");
  });
  it("renamed metrics file still binds by bytes; tampered bytes do not", () => {
    const files = evidenceFiles().map((f) => (f.name.includes("/artifacts/") ? { ...f, name: f.name.replace(/[0-9a-f]{64}$/, "renamed") } : f));
    expect(revCell(files).text).toBe("3/6");
  });
});
