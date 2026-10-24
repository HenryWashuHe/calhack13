// Eric's Claude: the judge-facing board (docs/invention-lab/ui.md#results-board):
// the A → B → A pass as a step diagram, the same steps in plain sentences, the
// results matrix, and every record below. Everything shown is derived from the
// supplied retained records; nothing here is a benefit claim.

import { LineagePanel } from "./LineagePanel";
import type { Lineage } from "./lineage";
import { buildMatrix, cellText } from "./matrix";
import { story, type Step } from "./story";

const W = 168;
const GAP = 28;
const H = 96;

function tone(word: string): string {
  return word === "accepted" || word === "adopted" ? "var(--ok)" : word === "rejected" ? "var(--bad)" : "var(--ink-3)";
}

function StepDiagram({ steps }: { steps: Step[] }) {
  const width = steps.length * W + (steps.length - 1) * GAP;
  return (
    <svg viewBox={`0 0 ${width} ${H + 8}`} width="100%" role="img" aria-label="A to B to A steps" style={{ maxWidth: width }}>
      {steps.map((s, i) => {
        const x = i * (W + GAP);
        const missing = s.state === "missing";
        return (
          <g key={s.title}>
            {i > 0 && (
              <path d={`M${x - GAP + 4} ${H / 2} H${x - 6} m-6 -5 l6 5 l-6 5`} fill="none" stroke="var(--ink-3)" strokeWidth={1.5} />
            )}
            <rect
              x={x} y={4} width={W} height={H} rx={8}
              fill={missing ? "var(--paper)" : "var(--panel)"}
              stroke={missing ? "var(--rule-strong)" : "var(--accent)"}
              strokeDasharray={missing ? "5 4" : undefined}
            />
            <text x={x + 12} y={28} fontSize={13} fontWeight={600} fill="var(--ink)">{s.title}</text>
            <text x={x + 12} y={50} fontSize={11.5} fill={missing ? "var(--ink-3)" : "var(--ink-2)"}>{s.figure}</text>
            {s.verdicts.map((v, j) => (
              <circle key={j} cx={x + 18 + j * 16} cy={76} r={6} fill={tone(v)}>
                <title>{v}</title>
              </circle>
            ))}
          </g>
        );
      })}
    </svg>
  );
}

export function ResultsBoard({ lineage, orgA, orgB }: { lineage: Lineage; orgA: string; orgB: string }) {
  const steps = story(lineage, orgA, orgB);
  const matrix = buildMatrix(lineage);
  const label = (key: string, id: string) => {
    // A revision is also generated; name it by its parent link first.
    const revised = lineage.edges.find((e) => e.to === key && e.kind === "revised_from");
    if (revised) return `${revised.org_id}'s revision`;
    const generated = lineage.edges.find((e) => e.to === key && e.kind === "generated");
    if (generated) return `${generated.org_id}'s generated method`;
    return `control: ${id}`;
  };
  return (
    <section className="lab-board">
      <h2>
        A → B → A records ({orgA} as A, {orgB} as B)
      </h2>
      <StepDiagram steps={steps} />
      <p className="lab-limit">Dots are trial verdicts: green accepted, red rejected. Dashed steps have no record.</p>
      <ol>
        {steps.map((s) => (
          <li key={s.title} data-state={s.state}>
            {s.text}
          </li>
        ))}
      </ol>
      <h3>Successes per task family</h3>
      <table>
        <thead>
          <tr>
            <th scope="col">Method</th>
            {matrix.families.map((f) => (
              <th scope="col" key={f}>{f}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {matrix.rows.map((r) => (
            <tr key={r.method.key}>
              <th scope="row">
                {label(r.method.key, r.method.ref.id)} <code>{r.method.ref.sha256.slice(0, 8)}</code>
              </th>
              {matrix.families.map((f) => (
                <td key={f} title={(r.cells.get(f) ?? []).map((x) => `${x.trial} (${x.role}, ${x.org}): ${x.verdict?.verdict ?? "no verdict"}`).join("\n")}>
                  {cellText(r.cells.get(f))}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      <p className="lab-limit">
        Counted from the scoring metrics each evaluation names by hash. Single exploratory runs on public development
        instances; verdicts are scoring evidence under frozen criteria, not adoption decisions, a ranking or a benefit
        claim.
      </p>
      <details>
        <summary>All records behind this board</summary>
        <LineagePanel lineage={lineage} orgA={orgA} orgB={orgB} />
      </details>
    </section>
  );
}
