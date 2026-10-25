// Eric's Claude: a method × task-family results matrix built only from retained
// records (docs/invention-lab/ui.md#results-matrix). Each run's successes come
// from the score lineage.ts resolved from its evaluations and the metrics
// artifact they name by hash; the trial verdict is shown as recorded. Runs are
// exploratory and per panel: not a ranking, a G4 comparison or a benefit claim.

import type { Lineage, MethodNode, Score } from "./lineage";
import { refKey, USAGE_DIMENSIONS, type ExactRef, type Verdict } from "./records";

/** Which instances a run was scored on: the plan's public development panel, a
 * hidden committed panel, or not established by the supplied records. */
export type Panel = "development" | "hidden" | "unknown";

export interface RunResult {
  /** The run's trial, named by plan id and exact hash prefix, or "ordinary run". */
  trial: string;
  /** The exact plan the run's purpose names, when it names one. */
  plan: ExactRef | null;
  role: string;
  org: string;
  score: Score;
  /** Only for a counted score; null otherwise. */
  successes: number | null;
  of: number | null;
  panel: Panel;
  /** Why the run cannot be read as a result (execution outcome or unmeasured
   * usage), or null when it can. */
  ineligible: string | null;
  verdict: Verdict | null;
}

export interface MatrixRow {
  method: MethodNode;
  cells: Map<string, RunResult[]>;
}

export interface Matrix {
  /** Column keys: the task id, or id@version with a hash prefix when one id
   * appears under several exact references. */
  families: string[];
  familyRefs: Map<string, ExactRef>;
  rows: MatrixRow[];
}

export function buildMatrix(l: Lineage): Matrix {
  const ids = new Map<string, Set<string>>();
  for (const e of l.executions) {
    const t = e.receipt.public_task;
    ids.set(t.id, (ids.get(t.id) ?? new Set()).add(refKey(t)));
  }
  const column = (t: ExactRef) => (ids.get(t.id)!.size > 1 ? `${t.id}@${t.version} ${t.sha256.slice(0, 8)}` : t.id);

  const families: string[] = [];
  const familyRefs = new Map<string, ExactRef>();
  const rows = new Map<string, MatrixRow>();
  for (const e of l.executions) {
    const family = column(e.receipt.public_task);
    if (!families.includes(family)) families.push(family);
    familyRefs.set(family, e.receipt.public_task);
    const node = l.nodes.find((n) => n.key === e.method);
    if (!node) continue;
    const row = rows.get(node.key) ?? { method: node, cells: new Map<string, RunResult[]>() };
    rows.set(node.key, row);
    const p = e.receipt.purpose;
    const plan = p.kind === "acceptance" || p.kind === "comparison" ? p.plan : null;
    const trials = l.trials.filter((t) => t.executions.includes(e));
    const trial = trials.find((t) => t.verdict) ?? trials[0];
    const scoredOn = e.evaluations.find((v) => v.outcome === "scored")?.context.instance_set?.id;
    const results = row.cells.get(family) ?? [];
    results.push({
      trial: plan ? `${plan.id} @ ${plan.sha256.slice(0, 8)}` : "ordinary run",
      plan,
      role: p.kind === "acceptance" ? p.role : p.kind === "comparison" ? `arm ${p.arm_id}` : p.kind === "ordinary" ? "run" : p.raw,
      org: e.receipt.org_id,
      score: e.score,
      successes: e.score.kind === "counted" ? e.score.successes : null,
      of: e.score.kind === "counted" ? e.score.of : null,
      panel: trial
        ? trial.plan.scoring_kind === "development"
          ? "development"
          : trial.plan.scoring_kind === "hidden_commitment"
            ? "hidden"
            : "unknown"
        : scoredOn === "development"
          ? "development"
          : "unknown",
      ineligible: ineligibility(e.receipt.outcome, e.receipt.usage),
      verdict: trial?.verdict ?? null,
    });
    row.cells.set(family, results);
  }
  return { families, familyRefs, rows: [...rows.values()] };
}

/** The process-slot eligibility rule (`assess_process_slot`): the execution
 * generated its output and measured all five usage dimensions. Scoring-grant
 * checks need the evaluation budget, which this view does not decode. */
function ineligibility(outcome: string, usage: Lineage["executions"][number]["receipt"]["usage"]): string | null {
  if (outcome !== "output_generated") return `execution ${outcome}`;
  const unmeasured = USAGE_DIMENSIONS.filter((d) => usage[d].kind !== "measured");
  return unmeasured.length > 0 ? `usage not measured: ${unmeasured.join(", ")}` : null;
}

/** One run's label: `1/6`, `1/12 hidden`, `12/12 (ineligible)`, or the reason
 * there is no count. Never zero for a missing score. */
export function runText(r: RunResult): string {
  const s = r.score;
  let text: string;
  switch (s.kind) {
    case "counted":
      text = `${s.successes}/${s.of}${r.panel === "hidden" ? " hidden" : ""}`;
      break;
    case "unscored":
      text = "not scored";
      break;
    case "not_scored":
      text = s.outcomes.map((o) => (o === "invalid" ? "output invalid" : `scoring ${o}`)).join(" / ");
      break;
    case "metrics_missing":
      text = "metrics not supplied";
      break;
    case "rejected":
      text = "metrics rejected";
      break;
    case "conflict":
      text = "conflicting scores";
      break;
  }
  return r.ineligible ? `${text} (ineligible)` : text;
}

/** The distinct run labels of a cell (`1/6 · 4/12 · 1/12 hidden`), or "–". Runs
 * with identical labels are listed once here; a view that shows runs must use
 * the runs themselves. */
export function cellText(runs: RunResult[] | undefined): string {
  if (!runs || runs.length === 0) return "–";
  return [...new Set(runs.map(runText))].join(" · ");
}
