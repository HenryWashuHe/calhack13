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
function evaluation(id: string, output: ExactRef, purpose: object, outcome: object) {
  return {
    request: { request_id: id, execution_sha256: "e".repeat(64), output, context: {}, purpose, budget: {} },
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
  it("attach each evaluation to the one run with that exact output and purpose", () => {
    const l = buildLineage({
      generations: [],
      executions: [
        decodeExecutionReceipt(execution({ run: "cand", method, purpose: candidate, output: designA })),
        decodeExecutionReceipt(execution({ run: "ctrl", method: seed, purpose: control, output: designC })),
      ],
      evaluations: [
        decodeEvaluationReceipt(evaluation("ev-cand", designA, candidate, { kind: "scored", metrics: ref("metrics", "m") })),
        decodeEvaluationReceipt(evaluation("ev-ctrl", designC, control, { kind: "unknown", reason: "scorer interrupted" })),
      ],
    });
    expect(l.problems).toEqual([]);
    expect(l.nodes.map((n) => n.ref.id).sort()).toEqual(["a-method", "seed-control"]);
    expect(l.executions.map((e) => [e.receipt.run_id, e.evaluations.map((v) => v.request_id)])).toEqual([
      ["cand", ["ev-cand"]],
      ["ctrl", ["ev-ctrl"]],
    ]);
  });

  it("report, rather than attach, an evaluation whose purpose differs from the run", () => {
    const l = buildLineage({
      generations: [],
      executions: [decodeExecutionReceipt(execution({ run: "cand", method, purpose: candidate, output: designA }))],
      evaluations: [decodeEvaluationReceipt(evaluation("ev-x", designA, control, { kind: "scored", metrics: ref("m", "m") }))],
    });
    expect(l.executions[0]!.evaluations).toEqual([]);
    expect(l.problems).toEqual(["evaluation ev-x matches 0 executions by output and purpose; not attached"]);
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
