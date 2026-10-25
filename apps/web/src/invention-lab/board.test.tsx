// Eric's Claude: the results board and its step story on each frozen
// first-pass evidence folder, plus the empty case, the decision filters and the
// single-file export. Static render, plus one export loaded in jsdom.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { JSDOM } from "jsdom";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { intake } from "./intake";
import { firstPassEvidenceFiles as evidenceFiles } from "./test-fixtures/first-pass";
import { buildLineage, type LabRecords } from "./lineage";
import { decodeGenerationStatus, decodeRevisionObservation, type ExactRef } from "./records";
import { ResultsBoard } from "./ResultsBoard";
import { story } from "./story";
import aStatus from "../../../../docs/invention-lab/evidence/live-method-20261024-generated/final-status.json";
import bStatus from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/final-status.json";
import revision from "../../../../docs/invention-lab/evidence/live-method-20261024-b-revision/revision-observation.json";

const textOf = (html: string) => html.replace(/<[^>]+>/g, " ").replace(/&#x27;/g, "'").replace(/\s+/g, " ");
const A = "org_example_a";
const B = "org_example_b";

describe("the first A → B → A pass", () => {
  const l = buildLineage(intake(evidenceFiles()).records);

  it("tells each step from the records, ending without a decision", () => {
    expect(story(l, A, B).map((s) => [s.state, s.figure, s.text])).toEqual([
      ["recorded", "3 attempts · 1 package", `${A} recorded 3 generation attempts: 3 sent to the model; 1 produced a package (model requests: 3 model calls, 12027 input / 18315 output tokens, failures included).`],
      ["recorded", "8 trials · 2 accepted, 2 rejected, 2 ineligible, 2 no verdict", `${B} tested ${A}'s method in 8 trials on ravine-crossing, rough-transport (4 scored on hidden committed panels): 2 accepted, 2 rejected, 2 ineligible, 2 no verdict.`],
      ["recorded", "source changed", `${B} recorded a revision of ${A}'s method; the source bytes changed (1 model calls, 20798 input / 5741 output tokens; its own generation record).`],
      ["recorded", "3 trials · 1 accepted, 2 rejected", `${B} tested its revision in 3 trials on rough-transport: 1 accepted, 2 rejected.`],
      ["recorded", "2 trials · 2 rejected", `${A} tested ${B}'s revision in 2 trials on return-mix: 2 rejected.`],
      ["missing", "no decision", `No adoption decision by ${A} is recorded, so there is no accepted return yet; its trials of the revision read 2 rejected.`],
    ]);
  });

  it("renders the diagram, per-run matrix and limits without an improvement claim", () => {
    const html = renderToStaticMarkup(<ResultsBoard lineage={l} orgA={A} orgB={B} />);
    const text = textOf(html);
    const visible = text.split("All records behind this board")[0]!;
    expect((html.match(/<rect /g) ?? []).length).toBe(6);
    expect((html.match(/stroke-dasharray/g) ?? []).length).toBe(1);
    expect((html.match(/<circle /g) ?? []).length).toBe(13);
    expect(text).toContain("Method ravine-crossing return-mix rough-transport");
    expect(visible).toContain("org_example_a's generated method 8215c367 1/6 example-ravine-crossing-acceptance @ 5e111f9a");
    expect(visible).toContain("1/12 hidden (ineligible) hidden-st-ravine-crossing-comparison @ e1442f29");
    expect(visible).toContain("org_example_b's revision 4f455dc6 –");
    // Random search is the candidate in its own reference trial, so it is not a control by origin.
    expect(visible).toContain("origin not in these files: random-search 06777e8e");
    expect(visible).toContain("control: fixed-cart dc5b74f3");
    expect(visible).toContain("not adoption decisions, a ranking or a benefit claim");
    expect(visible).not.toMatch(/problem/i);
    expect(visible).not.toMatch(/improv|better than|wins?\b|asked the model/i);
  });
});

it("marks every step missing when there are no records", () => {
  const l = buildLineage({ generations: [] });
  expect(story(l, A, B).every((s) => s.state === "missing")).toBe(true);
  const html = renderToStaticMarkup(<ResultsBoard lineage={l} orgA={A} orgB={B} />);
  expect((html.match(/stroke-dasharray/g) ?? []).length).toBe(6);
});

describe("revision and decision steps take only A's and B's own records", () => {
  const a = decodeGenerationStatus(aStatus);
  const b = decodeGenerationStatus(bStatus);
  const obs = decodeRevisionObservation(revision);
  const decided = (records: Partial<LabRecords>) =>
    story(buildLineage({ generations: [{ status: a }, { status: b }], revisions: [obs], ...records }), a.org_id, b.org_id);
  const other: ExactRef = { ...obs.child, id: "something-else", sha256: "e".repeat(64) };

  it("records A's decision on B's revision, and no one else's", () => {
    const declined = { outcome: "declined", reason: "local checks", installed: null };
    expect(decided({ adoptions: [{ receiver_org: a.org_id, offer: obs.child, outcome: declined }] })[5]).toMatchObject({
      state: "recorded",
      text: `${a.org_id} recorded 1 adoption decision on ${b.org_id}'s revision.`,
      verdicts: ["declined"],
    });
    expect(decided({ adoptions: [{ receiver_org: b.org_id, offer: obs.child, outcome: declined }] })[5]!.state).toBe("missing");
    expect(decided({ adoptions: [{ receiver_org: a.org_id, offer: other, outcome: declined }] })[5]!.state).toBe("missing");
  });

  it("does not count another organization's revision of A's method as B's", () => {
    const step = story(
      buildLineage({ generations: [{ status: a }], revisions: [{ ...obs, org_id: "org_c" }] }),
      a.org_id,
      b.org_id,
    )[2]!;
    expect(step.state).toBe("missing");
  });
});

it("shows lineage problems above the collapsed records", () => {
  const files = evidenceFiles();
  const ev = files.find((f) => f.name === "live-method-20261024-receiver/candidate-score/evaluation.json")!;
  const dangling = JSON.parse(ev.text);
  dangling.request.execution_sha256 = "0".repeat(64);
  const l = buildLineage(intake([...files, { name: "dangling/evaluation.json", text: JSON.stringify(dangling) }]).records);
  const visible = textOf(renderToStaticMarkup(<ResultsBoard lineage={l} orgA={A} orgB={B} />)).split("All records behind this board")[0]!;
  expect(visible).toContain("1 problem in these records");
  expect(visible).toContain("which no supplied receipt has");
});

it("exports one page that an evidence file cannot blank", { timeout: 60_000 }, async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lab-export-"));
  try {
    const evidence = path.join(dir, "evidence");
    fs.mkdirSync(path.join(evidence, "zz"), { recursive: true });
    fs.writeFileSync(path.join(evidence, "zz", "note.json"), JSON.stringify({ note: "<!--<script></script>" }));
    const out = path.join(dir, "board.html");
    const script = fileURLToPath(new URL("./export-board.mjs", import.meta.url));
    const run = spawnSync(process.execPath, [script, "--evidence", evidence, "--out", out], { encoding: "utf8" });
    expect(run.status, run.stderr).toBe(0);
    const html = fs.readFileSync(out, "utf8");
    const data = html.slice(html.indexOf('id="lab-evidence">'), html.indexOf("</script>"));
    expect(data).not.toContain("<!--");
    const errors: unknown[] = [];
    const dom = new JSDOM(html, {
      runScripts: "dangerously",
      beforeParse: (w) => {
        // Browsers provide these; jsdom does not.
        Object.assign(w, { TextEncoder, TextDecoder });
        w.addEventListener("error", (e) => errors.push(e.error));
      },
    });
    await new Promise((r) => setTimeout(r, 300));
    expect(errors).toEqual([]);
    expect(dom.window.document.getElementById("root")!.textContent).toContain("No generation attempt by org_example_a is recorded.");
    dom.window.close();
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
