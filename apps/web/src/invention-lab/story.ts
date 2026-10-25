// Eric's Claude: the A → B → A pass told in steps, each derived from retained
// records (docs/invention-lab/ui.md#results-board). A step without records says
// so. Verdicts are scoring evidence, not adoption decisions; no step claims
// improvement.

import { trialsOf } from "./beats";
import { buildLedger, totalText } from "./ledger";
import type { Lineage, Trial } from "./lineage";

export type StepState = "recorded" | "missing";
export interface Step {
  title: string;
  /** Short figures for the diagram, e.g. "3 attempts · 1 package". */
  figure: string;
  /** One plain sentence for the story list. */
  text: string;
  state: StepState;
  /** Verdict words of the trials in this step, in order. */
  verdicts: string[];
}

function verdictWords(trials: Trial[]): string[] {
  return trials.map((t) => t.verdict?.verdict ?? "no verdict");
}
function tally(words: string[]): string {
  const counts = new Map<string, number>();
  for (const w of words) counts.set(w, (counts.get(w) ?? 0) + 1);
  return [...counts].map(([w, n]) => `${n} ${w}`).join(", ");
}
function families(trials: Trial[]): string {
  return [...new Set(trials.flatMap((t) => t.executions.map((e) => e.receipt.public_task.id)))].join(", ");
}

export function story(l: Lineage, a: string, b: string): Step[] {
  const generatedBy = (org: string) =>
    new Set(l.edges.flatMap((e) => (e.kind === "generated" && e.org_id === org ? [e.to] : [])));
  const aMethods = generatedBy(a);
  const revisions = l.edges.flatMap((e) => (e.kind === "revised_from" && e.org_id === b && aMethods.has(e.from) ? [e] : []));
  const bRevisions = new Set(revisions.map((r) => r.to));
  const aAttempts = l.attempts.filter((x) => x.status.org_id === a);
  const bAttempts = l.attempts.filter((x) => x.status.org_id === b);
  const decisions = l.edges.filter((e) => e.kind === "adoption" && e.org_id === a && bRevisions.has(e.to));
  const bTrials = trialsOf(l, b, aMethods);
  const bRevTrials = trialsOf(l, b, bRevisions);
  const aTrials = trialsOf(l, a, bRevisions);
  const cost = (attempts: typeof aAttempts) => {
    const t = buildLedger(attempts).totals;
    return `${totalText(t.model_calls)} model calls, ${totalText(t.input_tokens)} input / ${totalText(t.output_tokens)} output tokens`;
  };
  const trialStep = (title: string, who: string, trials: Trial[], what: string): Step =>
    trials.length
      ? {
          title,
          figure: `${trials.length} trials · ${tally(verdictWords(trials))}`,
          text: `${who} tested ${what} in ${trials.length} trial${trials.length === 1 ? "" : "s"} on ${families(trials)}: ${tally(verdictWords(trials))}.`,
          state: "recorded",
          verdicts: verdictWords(trials),
        }
      : { title, figure: "no record", text: `No record of ${who} testing ${what}.`, state: "missing", verdicts: [] };

  const packages = aAttempts.filter((x) => x.status.package).length;
  return [
    aAttempts.length
      ? {
          title: `${a} generates`,
          figure: `${aAttempts.length} attempts · ${packages} package${packages === 1 ? "" : "s"}`,
          text: `${a} asked the model for a development method ${aAttempts.length} times; ${packages} attempt${packages === 1 ? "" : "s"} produced a package (${cost(aAttempts)}, failures included).`,
          state: "recorded",
          verdicts: [],
        }
      : { title: `${a} generates`, figure: "no record", text: `No generation attempt by ${a} is recorded.`, state: "missing", verdicts: [] },
    trialStep(`${b} tests it`, b, bTrials, `${a}'s method`),
    revisions.length
      ? {
          title: `${b} revises`,
          figure: revisions.some((r) => r.kind === "revised_from" && r.source_changed) ? "source changed" : "source unchanged",
          text: `${b} generated a revision of ${a}'s method from its feedback (${cost(bAttempts)}); the source bytes ${revisions.some((r) => r.kind === "revised_from" && r.source_changed) ? "changed" : "did not change"}.`,
          state: "recorded",
          verdicts: [],
        }
      : { title: `${b} revises`, figure: "no record", text: `No revision by ${b} is recorded.`, state: "missing", verdicts: [] },
    trialStep(`${b} tests the revision`, b, bRevTrials, "its revision"),
    trialStep(`${a} tests it on return`, a, aTrials, `${b}'s revision`),
    decisions.length
      ? {
          title: `${a} decides`,
          figure: `${decisions.length} decision${decisions.length === 1 ? "" : "s"}`,
          text: `${a} recorded ${decisions.length} adoption decision${decisions.length === 1 ? "" : "s"} on ${b}'s revision.`,
          state: "recorded",
          verdicts: decisions.map((d) => (d.kind === "adoption" ? d.outcome.outcome : "")),
        }
      : {
          title: `${a} decides`,
          figure: "no decision",
          text: `No adoption decision by ${a} is recorded, so there is no accepted return yet${aTrials.length ? `; its trials of the revision read ${tally(verdictWords(aTrials))}` : ""}.`,
          state: "missing",
          verdicts: [],
        },
  ];
}
