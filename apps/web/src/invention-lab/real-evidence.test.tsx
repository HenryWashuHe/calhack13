// Eric's Claude: the receiving-side run (#98) and B's feedback-driven revision
// (#112/#113), read from the committed evidence. Package bytes are rebuilt from
// their JSON imports; the first cases prove they hash to the recorded refs.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { beats } from "./beats";
import { intake } from "./intake";
import { buildLineage } from "./lineage";
import { LineagePanel } from "./LineagePanel";
import { decodeTrialPlan } from "./records";
import aStatus from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/final-status.json";
import aPackage from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/method-package.json";
import bStatus from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/final-status.json";
import bStep from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/step-result.json";
import bPackage from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/method-package.json";
import bParent from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/parent-package.json";
import bRevision from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/revision-observation.json";
import bInspection from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/static-inspection.json";
import plan from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/plan.json";
import verdict from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/verdict.json";
import cand from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/candidate/receipt.json";
import candScore from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/candidate-score/evaluation.json";
import ctrl from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/control/receipt.json";
import ctrlScore from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/control-score/evaluation.json";
import ctrlPackage from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/control.package.json";
import rsPlan from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/vs-random-search/plan.json";
import rsVerdict from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/vs-random-search/verdict.json";
import rsCand from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/vs-random-search/candidate/receipt.json";
import rsCandScore from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/vs-random-search/candidate-score/evaluation.json";
import rsCtrl from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/vs-random-search/control/receipt.json";
import rsCtrlScore from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/vs-random-search/control-score/evaluation.json";
import refPlan from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/reference-random-search/plan.json";
import refCand from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/reference-random-search/candidate/receipt.json";
import rsPackage from "../../../../docs/invention-lab/evidence/live-method-20261024-receiver/reference-random-search/rs.package.json";

const R = "live-method-20261024-receiver";
const B = "live-method-20261024-b-revision";
const file = (name: string, v: unknown) => ({ name, text: JSON.stringify(v) });
const textOf = (html: string) => html.replace(/<[^>]+>/g, " ").replace(/&#x27;/g, "'").replace(/\s+/g, " ");
const files = [
  file("live-method-20261024-generated/final-status.json", aStatus),
  file("live-method-20261024-generated/method-package.json", aPackage),
  file(`${B}/final-status.json`, bStatus),
  file(`${B}/step-result.json`, bStep),
  file(`${B}/method-package.json`, bPackage),
  file(`${B}/parent-package.json`, bParent),
  file(`${B}/revision-observation.json`, bRevision),
  file(`${B}/static-inspection.json`, bInspection),
  file(`${R}/plan.json`, plan),
  file(`${R}/verdict.json`, verdict),
  file(`${R}/candidate/receipt.json`, cand),
  file(`${R}/candidate-score/evaluation.json`, candScore),
  file(`${R}/control/receipt.json`, ctrl),
  file(`${R}/control-score/evaluation.json`, ctrlScore),
  file(`${R}/control.package.json`, ctrlPackage),
  file(`${R}/vs-random-search/plan.json`, rsPlan),
  file(`${R}/vs-random-search/verdict.json`, rsVerdict),
  file(`${R}/vs-random-search/candidate/receipt.json`, rsCand),
  file(`${R}/vs-random-search/candidate-score/evaluation.json`, rsCandScore),
  file(`${R}/vs-random-search/control/receipt.json`, rsCtrl),
  file(`${R}/vs-random-search/control-score/evaluation.json`, rsCtrlScore),
  file(`${R}/reference-random-search/plan.json`, refPlan),
  file(`${R}/reference-random-search/candidate/receipt.json`, refCand),
  file(`${R}/reference-random-search/rs.package.json`, rsPackage),
];

describe("retained receiving run and B revision", () => {
  const i = intake(files);
  const l = buildLineage(i.records);

  it("reads every record with no problems, counting identical copies once", () => {
    expect(l.problems).toEqual([]);
    expect(i.skipped).toEqual([
      { file: `${B}/step-result.json`, note: `identical to ${B}/final-status.json; counted once` },
      { file: `${B}/parent-package.json`, note: "identical to live-method-20261024-generated/method-package.json; counted once" },
    ]);
    expect(l.nodes.map((n) => `${n.ref.id} ${n.ref.sha256.slice(0, 12)} ${n.package ? "bytes" : "-"}`).sort()).toEqual([
      "fixed-cart dc5b74f34698 bytes",
      "generated-method 8215c3679a06 bytes",
      "random-search 06777e8ed363 bytes",
      "receiver-revised-method 4f455dc6e625 bytes",
    ]);
  });

  it("computes trial plan references that match what the receipts name", () => {
    expect(decodeTrialPlan(plan).ref.sha256).toBe(cand.binding.purpose.plan.sha256);
    expect(decodeTrialPlan(rsPlan).ref.sha256).toBe(rsCand.binding.purpose.plan.sha256);
    expect(decodeTrialPlan(refPlan).ref.sha256).toBe(refCand.binding.purpose.plan.sha256);
  });

  it("files runs that reuse run ids under their own trials, each with its named evaluation", () => {
    expect(l.trials.map((t) => [t.plan.ref.sha256.slice(0, 10), t.verdict?.verdict ?? null, t.executions.map((e) => `${e.receipt.actual_method.id}:${e.evaluations.length}`)])).toEqual([
      ["5e111f9a4f", "accepted", ["generated-method:1", "fixed-cart:1"]],
      ["7a1dd808c5", "rejected", ["generated-method:1", "random-search:1"]],
      ["e5e95a4c91", null, ["random-search:0"]],
    ]);
  });

  it("reads the second static-inspection shape verbatim and checks it against B's package", () => {
    const n = l.nodes.find((x) => x.ref.id === "receiver-revised-method")!;
    expect(n.inspections).toEqual([
      {
        method_package_sha256: "4f455dc6e625bdaf554d567bdd7f0bc0c7b601a1ea6332d4e7a13e02005db14d",
        entrypoint: null,
        source_sha256: "4adde0d840bcb733166c3db7f587fd295f1f986369433ea3db58e40be1d76de5",
        parse: "ast_parse true",
        imports: ["json", "math", "sys"],
        execution: "source_executed false",
        claim: "syntax and bytes only, not execution or correctness",
      },
    ]);
    expect(n.package!.files).toEqual([{ path: "method.py", bytes: 6044, lines: 192, sha256: n.inspections[0]!.source_sha256 }]);
  });

  it("shows B's trials of A's method as verdicts, not decisions, and nothing yet for A's return", () => {
    const b = beats(l, "org_example_a", "org_example_b");
    expect(b[1]!.facts.map((f) => [f.recorded, f.text])).toEqual([
      [false, "org_example_b decisions on org_example_a's methods: none recorded"],
      [true, "org_example_b runs: output generated (1 evaluation); output generated (1 evaluation)"],
      [
        true,
        "org_example_b trials of org_example_a's methods: example-ravine-crossing-acceptance: accepted: candidate 1/6 vs control 0/6; " +
          "example-ravine-crossing-generated-vs-random-search: rejected: candidate 1/6 does not exceed control 2/6 by 1 (trial verdicts, not adoption decisions)",
      ],
    ]);
    expect(b[2]!.facts[0]).toEqual({ text: "org_example_b revisions of org_example_a's methods: source changed (bytes)", recorded: true });
    expect(b[3]!.facts.every((f) => !f.recorded)).toBe(true);
    expect(b[4]!.facts[0]!.text).toBe("chain incomplete; no record of org_example_a's decision on that revision");
  });

  it("renders the trials with their pairing note and the adoption limit", () => {
    const text = textOf(renderToStaticMarkup(<LineagePanel lineage={l} orgA="org_example_a" orgB="org_example_b" />));
    expect(text).toContain("accepted: candidate 1/6 vs control 0/6 (paired by folder; the verdict names no plan)");
    expect(text).toContain("acceptance trial, candidate slot: generated-method run by org_example_b, output generated, 1 evaluation");
    expect(text).toContain("A trial verdict is scoring evidence under its frozen criterion, not an adoption decision.");
    expect(text).toContain("ast_parse true; imports json, math, sys; source_executed false.");
    expect(text.split("Generation attempts")[0]).not.toMatch(/improv|better/i);
  });
});

describe("verdict pairing", () => {
  it("does not guess a plan for a verdict with none or several in its folder", () => {
    const none = intake([file("x/verdict.json", verdict)]);
    const two = intake([file("y/a.json", plan), file("y/b.json", rsPlan), file("y/verdict.json", verdict)]);
    expect(none.skipped).toEqual([{ file: "x/verdict.json", note: "trial verdict with 0 trial plans in its folder; names no plan, so not paired" }]);
    expect(two.skipped.map((n) => n.note)).toEqual(["trial verdict with 2 trial plans in its folder; names no plan, so not paired"]);
    expect(two.records.trials!.map((t) => t.verdict)).toEqual([null, null]);
  });
});

describe("trial roles", () => {
  it("credits a trial to the method run as its candidate, not to a method used as the control", () => {
    const revised = { domain: "genesis", schema: "development-method-source/1", id: "receiver-revised-method", version: "0.1.0", sha256: "4f455dc6e625bdaf554d567bdd7f0bc0c7b601a1ea6332d4e7a13e02005db14d" };
    const parent = cand.actual_method;
    const vsParent = { ...structuredClone(plan), id: "revision-vs-parent" };
    const ref = decodeTrialPlan(vsParent).ref;
    const run = (r: typeof cand, method: typeof parent) => {
      const x = structuredClone(r);
      x.binding.method = method;
      x.actual_method = method;
      x.binding.purpose.plan = ref;
      return x;
    };
    const l = buildLineage(
      intake([
        ...files,
        file("z/plan.json", vsParent),
        file("z/verdict.json", { verdict: "rejected", reason: "synthetic" }),
        file("z/candidate.json", run(cand, revised)),
        file("z/control.json", run(ctrl, parent)),
      ]).records,
    );
    const b = beats(l, "org_example_a", "org_example_b");
    expect(b[1]!.facts[2]!.text).not.toContain("revision-vs-parent");
    expect(b[2]!.facts[1]!.text).toBe("org_example_b runs: output generated (0 evaluations)");
  });
});
