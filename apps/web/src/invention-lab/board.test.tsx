// Eric's Claude: the results board and its step story on every committed
// evidence folder, plus the empty case. Static render only.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { intake } from "./intake";
import { buildLineage } from "./lineage";
import { ResultsBoard } from "./ResultsBoard";
import { story } from "./story";

const root = fileURLToPath(new URL("../../../../docs/invention-lab/evidence/", import.meta.url));
function evidenceFiles() {
  const out: { name: string; text: string }[] = [];
  const walk = (dir: string) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (!/\.(md|txt)$|SHA256SUMS$/.test(e.name)) out.push({ name: path.relative(root, p), text: fs.readFileSync(p, "utf8") });
    }
  };
  walk(root);
  return out;
}
const textOf = (html: string) => html.replace(/<[^>]+>/g, " ").replace(/&#x27;/g, "'").replace(/\s+/g, " ");
const A = "org_example_a";
const B = "org_example_b";

describe("the first A → B → A pass", () => {
  const l = buildLineage(intake(evidenceFiles()).records);

  it("tells each step from the records, ending without a decision", () => {
    expect(story(l, A, B).map((s) => [s.state, s.figure, s.text])).toEqual([
      ["recorded", "3 attempts · 1 package", `${A} asked the model for a development method 3 times; 1 attempt produced a package (3 model calls, 12027 input / 18315 output tokens, failures included).`],
      ["recorded", "2 trials · 1 accepted, 1 rejected", `${B} tested ${A}'s method in 2 trials on ravine-crossing: 1 accepted, 1 rejected.`],
      ["recorded", "source changed", `${B} generated a revision of ${A}'s method from its feedback (1 model calls, 20798 input / 5741 output tokens); the source bytes changed.`],
      ["recorded", "3 trials · 1 accepted, 2 rejected", `${B} tested its revision in 3 trials on rough-transport: 1 accepted, 2 rejected.`],
      ["recorded", "2 trials · 2 rejected", `${A} tested ${B}'s revision in 2 trials on return-mix: 2 rejected.`],
      ["missing", "no decision", `No adoption decision by ${A} is recorded, so there is no accepted return yet; its trials of the revision read 2 rejected.`],
    ]);
  });

  it("renders the diagram, matrix and limits without an improvement claim", () => {
    const html = renderToStaticMarkup(<ResultsBoard lineage={l} orgA={A} orgB={B} />);
    const text = textOf(html);
    expect((html.match(/<rect /g) ?? []).length).toBe(6);
    expect((html.match(/stroke-dasharray/g) ?? []).length).toBe(1);
    expect((html.match(/<circle /g) ?? []).length).toBe(7);
    expect(text).toContain("ravine-crossing return-mix rough-transport");
    expect(text).toMatch(/org_example_a's generated method 8215c367 1\/6 1\/6 6\/6/);
    expect(text).toMatch(/org_example_b's revision 4f455dc6 – 0\/6 3\/6/);
    expect(text).toMatch(/control: random-search 06777e8e 2\/6 3\/6 4\/6/);
    expect(text).toContain("not adoption decisions, a ranking or a benefit claim");
    expect(text.split("All records behind this board")[0]).not.toMatch(/improv|better than|wins?\b/i);
  });
});

it("marks every step missing when there are no records", () => {
  const l = buildLineage({ generations: [] });
  expect(story(l, A, B).every((s) => s.state === "missing")).toBe(true);
  const html = renderToStaticMarkup(<ResultsBoard lineage={l} orgA={A} orgB={B} />);
  expect((html.match(/stroke-dasharray/g) ?? []).length).toBe(6);
});
