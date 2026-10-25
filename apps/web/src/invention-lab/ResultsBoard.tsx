// Eric's Claude: the judge-facing board (docs/invention-lab/ui.md#results-board):
// the A → B → A pass as a step diagram, the same steps in plain sentences, the
// results matrix, and every record below. Everything shown is derived from the
// supplied retained records; nothing here is a benefit claim.

import { LineagePanel } from "./LineagePanel";
import type { Lineage } from "./lineage";
import { buildMatrix, runText, type RunResult } from "./matrix";
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

/** A run's role in its own trial: only acceptance control slots and control
 * arms are controls. */
function isControl(r: RunResult): boolean {
  return r.role === "control" || r.role === "arm control";
}

function runTitle(r: RunResult): string {
  const verdict = r.verdict ? `trial verdict on its candidate: ${r.verdict.verdict}` : "no trial verdict supplied";
  const panel = r.panel === "hidden" ? "hidden committed panel" : r.panel === "development" ? "public development panel" : "panel not established";
  return [`${r.trial} · ${r.role} · ${r.org}`, panel, verdict, r.ineligible ? `ineligible: ${r.ineligible}` : null]
    .filter(Boolean)
    .join("\n");
}

export function ResultsBoard({ lineage, orgA, orgB }: { lineage: Lineage; orgA: string; orgB: string }) {
  const steps = story(lineage, orgA, orgB);
  const matrix = buildMatrix(lineage);
  const label = (key: string, id: string, runs: RunResult[]) => {
    // A revision is also generated; name it by its parent link first.
    const revised = lineage.edges.find((e) => e.to === key && e.kind === "revised_from");
    if (revised) return `${revised.org_id}'s revision`;
    const generated = lineage.edges.find((e) => e.to === key && e.kind === "generated");
    if (generated) return `${generated.org_id}'s generated method`;
    // Without an origin record, a method is a control only if every one of its
    // runs filled a control slot or arm; a role is per trial, not per method.
    return runs.length > 0 && runs.every(isControl) ? `control: ${id}` : `origin not in these files: ${id}`;
  };
  return (
    <section className="lab-board">
      <h2>
        A → B → A records ({orgA} as A, {orgB} as B)
      </h2>
      {lineage.problems.length > 0 && (
        <div className="lab-problems" role="alert" style={{ border: "1px solid var(--bad)", borderRadius: 6, padding: "8px 12px", margin: "8px 0" }}>
          <strong>
            {lineage.problems.length} problem{lineage.problems.length === 1 ? "" : "s"} in these records. The steps and
            results below are read with them excluded or marked:
          </strong>
          <ul>
            {lineage.problems.map((p, i) => (
              <li key={i}>{p}</li>
            ))}
          </ul>
        </div>
      )}
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
                {label(r.method.key, r.method.ref.id, [...r.cells.values()].flat())} <code>{r.method.ref.sha256.slice(0, 8)}</code>
              </th>
              {matrix.families.map((f) => {
                const runs = r.cells.get(f) ?? [];
                return (
                  <td key={f}>
                    {runs.length === 0 ? (
                      "–"
                    ) : (
                      <ul className="lab-runs" style={{ listStyle: "none", margin: 0, padding: 0 }}>
                        {runs.map((x, i) => (
                          <li key={i} title={runTitle(x)} data-panel={x.panel} data-eligible={x.ineligible ? "no" : "yes"}>
                            {runText(x)} <small>{x.trial}</small>
                          </li>
                        ))}
                      </ul>
                    )}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      <p className="lab-limit">
        One line per run, counted from the scoring metrics its evaluation names by hash. Runs marked hidden were scored on a
        hidden committed panel, the others on public development instances. Runs marked ineligible are retained but
        must not be read as results. Verdicts are scoring evidence under frozen criteria, not adoption decisions, a
        ranking or a benefit claim.
      </p>
      <details>
        <summary>All records behind this board</summary>
        <LineagePanel lineage={lineage} orgA={orgA} orgB={orgB} />
      </details>
    </section>
  );
}
