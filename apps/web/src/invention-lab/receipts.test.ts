// Eric's Claude: execution and evaluation receipts. No executed generated
// method exists yet, so these records are synthetic, shaped exactly like the
// genesis-core serializers (ExecutionReceipt / EvaluationReceipt).
import { describe, expect, it } from "vitest";
import { evaluationLabel, executionLabel, purposeText } from "./claims";
import { buildLineage } from "./lineage";
import { DecodeError, decodeEvaluationReceipt, decodeExecutionReceipt, refKey, type ExactRef } from "./records";

const ref = (id: string, c: string, schema = "development-method-source/1"): ExactRef => ({
  domain: "genesis-lab",
  schema,
  id,
  version: "1",
  sha256: c.repeat(64),
});
const usage = {
  model_calls: { kind: "unknown", reason: "sandbox cannot establish zero" },
  input_tokens: { kind: "measured", value: 0 },
  output_tokens: { kind: "measured", value: 0 },
  wall_time_ms: { kind: "measured", value: 812 },
  simulation_steps: { kind: "measured", value: 4000 },
};
const plan = ref("acceptance-plan", "p", "lab-trial-plan/1");
const candidate = { kind: "acceptance", plan, role: "candidate" };
const control = { kind: "acceptance", plan, role: "control" };

function execution(o: { run: string; method: ExactRef; ran?: ExactRef; purpose: object; outcome?: string; output: ExactRef }) {
  return {
    binding: {
      org_id: "org_b",
      run_id: o.run,
      attempt_id: "att_1",
      method: o.method,
      dependencies: [],
      public_task: ref("ravine-crossing", "t", "lab-task-description/1"),
      evaluator: ref("evaluator", "e"),
      model_settings: ref("settings", "s"),
      environment: ref("python3", "r"),
      purpose: o.purpose,
      budget: { model_calls: 0, input_tokens: 0, output_tokens: 0, wall_time_ms: 60000, simulation_steps: 100000 },
    },
    binding_sha256: "b".repeat(64),
    actual_method: o.ran ?? o.method,
    actual_environment: ref("python3", "r"),
    outcome: o.outcome ?? "output_generated",
    outputs: [o.output],
    transcript: ref("transcript", "x"),
    usage,
  };
}
/** Names its execution the way the scorer does: by the decoded receipt's hash. */
function evaluation(id: string, output: ExactRef, purpose: object, outcome: object, of = "e".repeat(64)) {
  return {
    request: { request_id: id, execution_sha256: of, output, context: {}, purpose, budget: {} },
    request_sha256: "q".repeat(64),
    observed_context: null,
    outcome,
    trace: ref("trace", "z"),
    usage,
  };
}

const method = ref("a-method", "a");
const seed = ref("seed-control", "c");
const designA = ref("design", "1", "lab-design/1");
const designC = ref("design", "2", "lab-design/1");

describe("receipt decoders", () => {
  it("keep executor-reported method, purpose, outcome and unknown usage", () => {
    const r = decodeExecutionReceipt(execution({ run: "run-1", method, purpose: candidate, output: designA }));
    expect(r.org_id).toBe("org_b");
    expect(refKey(r.actual_method)).toBe(refKey(method));
    expect(purposeText(r.purpose)).toBe("acceptance trial, candidate slot");
    expect(executionLabel(r.outcome).text).toBe("output generated");
    expect(executionLabel(r.outcome).limit).toContain("not design success");
    expect(r.usage.model_calls).toEqual({ kind: "unknown", reason: "sandbox cannot establish zero" });
  });

  it("read metrics only from a scored outcome and reasons otherwise", () => {
    const m = ref("metrics", "m", "lab-metrics/1");
    const scored = decodeEvaluationReceipt(evaluation("ev-1", designA, candidate, { kind: "scored", metrics: m }));
    expect(scored.metrics).toEqual(m);
    expect(evaluationLabel(scored.outcome).limit).toContain("acceptance is a separate verdict");
    const invalid = decodeEvaluationReceipt(evaluation("ev-2", designA, candidate, { kind: "invalid", reason: "design schema" }));
    expect([invalid.metrics, invalid.reason]).toEqual([null, "design schema"]);
  });

  it("keep unexpected kinds visible and reject malformed receipts", () => {
    const r = decodeExecutionReceipt(execution({ run: "r", method, purpose: { kind: "rehearsal" }, outcome: "teleported", output: designA }));
    expect(purposeText(r.purpose)).toBe("unrecognized purpose: rehearsal");
    expect(executionLabel(r.outcome).text).toBe("unrecognized: teleported");
    expect(() => decodeExecutionReceipt({ ...execution({ run: "r", method, purpose: candidate, output: designA }), outputs: "x" })).toThrow(DecodeError);
  });
});

describe("executions in the lineage", () => {
  const cand = decodeExecutionReceipt(execution({ run: "cand", method, purpose: candidate, output: designA }));
  const ctrl = decodeExecutionReceipt(execution({ run: "ctrl", method: seed, purpose: control, output: designC }));

  it("attach each evaluation to the run its request names by receipt hash", () => {
    const l = buildLineage({
      generations: [],
      executions: [cand, ctrl],
      evaluations: [
        decodeEvaluationReceipt(evaluation("ev-cand", designA, candidate, { kind: "scored", metrics: ref("metrics", "m") }, cand.receipt_sha256)),
        decodeEvaluationReceipt(evaluation("ev-ctrl", designC, control, { kind: "unknown", reason: "scorer interrupted" }, ctrl.receipt_sha256)),
      ],
    });
    expect(l.problems).toEqual([]);
    expect(l.nodes.map((n) => n.ref.id).sort()).toEqual(["a-method", "seed-control"]);
    expect(l.executions.map((e) => [e.receipt.run_id, e.evaluations.map((v) => v.request_id)])).toEqual([
      ["cand", ["ev-cand"]],
      ["ctrl", ["ev-ctrl"]],
    ]);
  });

  it("report, rather than attach, evaluations that name no run, another output or another purpose", () => {
    const scored = { kind: "scored", metrics: ref("m", "m") };
    const l = buildLineage({
      generations: [],
      executions: [cand],
      evaluations: [
        decodeEvaluationReceipt(evaluation("ev-none", designA, candidate, scored)),
        decodeEvaluationReceipt(evaluation("ev-output", designC, candidate, scored, cand.receipt_sha256)),
        decodeEvaluationReceipt(evaluation("ev-purpose", designA, control, scored, cand.receipt_sha256)),
      ],
    });
    expect(l.executions[0]!.evaluations).toEqual([]);
    expect(l.problems).toEqual([
      "evaluation ev-none names execution eeeeeeeeeeee, which no supplied receipt has; not attached",
      "evaluation ev-output scores an output its named execution did not produce; not attached",
      "evaluation ev-purpose has a different purpose than its named execution; not attached",
    ]);
  });

  it("give every field of the receipt a part in its identity", () => {
    const other = decodeExecutionReceipt({ ...execution({ run: "cand", method, purpose: candidate, output: designA }), transcript: ref("transcript", "y") });
    expect(other.receipt_sha256).not.toBe(cand.receipt_sha256);
    expect(cand.receipt_sha256).toMatch(/^[0-9a-f]{64}$/);
  });

  it("files a run under the method it actually ran and reports the binding mismatch", () => {
    const l = buildLineage({
      generations: [],
      executions: [decodeExecutionReceipt(execution({ run: "swap", method, ran: seed, purpose: candidate, output: designA }))],
    });
    expect(l.executions[0]!.method).toBe(refKey(seed));
    expect(l.problems[0]).toMatch(/ran a different method than its binding named/);
  });

  it("report a duplicated run", () => {
    const e = decodeExecutionReceipt(execution({ run: "dup", method, purpose: candidate, output: designA }));
    expect(buildLineage({ generations: [], executions: [e, e] }).problems[0]).toMatch(/appears more than once/);
  });
});

it("labels inherited Object member names as unrecognized execution and evaluation outcomes", () => {
  for (const raw of ["constructor", "toString", "__proto__"]) {
    expect(executionLabel(raw).text).toBe(`unrecognized: ${raw}`);
    expect(evaluationLabel(raw).text).toBe(`unrecognized: ${raw}`);
  }
  expect([executionLabel("failed").text, evaluationLabel("scored").text]).toEqual(["execution failed", "scored"]);
});
