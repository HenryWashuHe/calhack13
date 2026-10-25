// Eric's Claude: a method × task-family results matrix built only from retained
// records (docs/invention-lab/ui.md#results-matrix). Each run's successes are
// counted from the scoring metrics artifact its evaluation names by hash; the
// trial verdict is shown as recorded. Single exploratory runs on public
// development instances: not a ranking, a G4 comparison or a benefit claim.

import type { Lineage, MethodNode } from "./lineage";
import type { Verdict } from "./records";

export interface RunResult {
  trial: string;
  role: string;
  org: string;
  /** Whether any evaluation of this run is supplied. */
  scored: boolean;
  /** Null when no scored evaluation with a supplied metrics artifact exists. */
  successes: number | null;
  of: number | null;
  verdict: Verdict | null;
}

export interface MatrixRow {
  method: MethodNode;
  cells: Map<string, RunResult[]>;
}

export interface Matrix {
  families: string[];
  rows: MatrixRow[];
}

export function buildMatrix(l: Lineage): Matrix {
  const families: string[] = [];
  const rows = new Map<string, MatrixRow>();
  const trialOf = new Map(l.trials.flatMap((t) => t.executions.map((e) => [e.receipt.receipt_sha256, t] as const)));
  for (const e of l.executions) {
    const family = e.receipt.public_task.id;
    if (!families.includes(family)) families.push(family);
    const node = l.nodes.find((n) => n.key === e.method);
    if (!node) continue;
    const row = rows.get(node.key) ?? { method: node, cells: new Map<string, RunResult[]>() };
    rows.set(node.key, row);
    const scored = e.evaluations.find((v) => v.metrics && l.metrics.has(v.metrics.sha256));
    const m = scored?.metrics ? l.metrics.get(scored.metrics.sha256) : undefined;
    const p = e.receipt.purpose;
    const trial = trialOf.get(e.receipt.receipt_sha256);
    const results = row.cells.get(family) ?? [];
    results.push({
      trial: trial ? trial.plan.ref.id : p.kind === "acceptance" || p.kind === "comparison" ? p.plan.id : "ordinary run",
      role: p.kind === "acceptance" ? p.role : p.kind === "comparison" ? `arm ${p.arm_id}` : p.kind === "ordinary" ? "run" : p.raw,
      org: e.receipt.org_id,
      scored: e.evaluations.length > 0,
      successes: m ? m.instances.filter((r) => r.success).length : null,
      of: m ? m.instances.length : null,
      verdict: trial?.verdict ?? null,
    });
    row.cells.set(family, results);
  }
  // Families in a fixed order; records carry no chronology to sort by.
  return { families: families.sort(), rows: [...rows.values()] };
}

/** "1/6", "1/6 · 2/6" for differing runs, "metrics not supplied", "not scored", or "–". */
export function cellText(runs: RunResult[] | undefined): string {
  if (!runs || runs.length === 0) return "–";
  const one = (r: RunResult) =>
    r.successes !== null ? `${r.successes}/${r.of}` : r.scored ? "metrics not supplied" : "not scored";
  const counts = [...new Set(runs.map(one))];
  return counts.join(" · ");
}
