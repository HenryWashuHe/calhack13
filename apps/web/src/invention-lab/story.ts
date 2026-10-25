// Eric's Claude: the A → B → A pass told in steps, each derived from retained
// records (docs/invention-lab/ui.md#results-board). A step without records says
// so. Verdicts are scoring evidence, not adoption decisions; no step claims
// improvement, and no step says more than its records establish: a declared
// slot is not a request, a fixture is not a model, and a revision's cost is
// only its own generation record's.

import { trialsOf } from "./beats";
import { tierOf } from "./claims";
import { buildLedger, totalText } from "./ledger";
import type { Attempt, Edge, Lineage, Trial } from "./lineage";
import { ineligibility } from "./matrix";
import { refKey } from "./records";

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

/** A trial's word: its recorded verdict, or "ineligible" when any of its runs
 * failed the process-slot rule (a comparison with an ineligible candidate or
 * control slot has no reading, whatever its verdict says), or "no verdict". */
export function verdictWords(trials: Trial[]): string[] {
  return trials.map((t) =>
    t.executions.some((e) => ineligibility(e.receipt.outcome, e.receipt.usage) !== null)
      ? "ineligible"
      : (t.verdict?.verdict ?? "no verdict"),
  );
}
function tally(words: string[]): string {
  const counts = new Map<string, number>();
  for (const w of words) counts.set(w, (counts.get(w) ?? 0) + 1);
  return [...counts].map(([w, n]) => `${n} ${w}`).join(", ");
}
function families(trials: Trial[]): string {
  return [...new Set(trials.flatMap((t) => t.executions.map((e) => e.receipt.public_task.id)))].join(", ");
}
const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
function hiddenNote(trials: Trial[]): string {
  const hidden = trials.filter((t) => t.plan.scoring_kind === "hidden_commitment").length;
  return hidden ? ` (${hidden} scored on hidden committed panels)` : "";
}

/** How an attempt reached (or did not reach) a model, from its own phase and transport. */
type Route = "declared" | "model" | "fixture" | "unrecorded";
function route(a: Attempt): Route {
  if (a.status.phase === "prepared") return "declared";
  const tier = tierOf(a.status.transport);
  return tier === "operator_observed_cloud" ? "model" : tier === "loopback_fixture" ? "fixture" : "unrecorded";
}
const ROUTE_TEXT: Record<Route, string> = {
  model: "sent to the model",
  fixture: "answered by a loopback fixture, not a model",
  declared: "declared but not sent",
  unrecorded: "with no recorded transport",
};

/** Model usage of the attempts that reached a model; fixture usage is not model use. */
function cost(attempts: Attempt[]): string {
  const t = buildLedger(attempts).totals;
  return `${totalText(t.model_calls)} model calls, ${totalText(t.input_tokens)} input / ${totalText(t.output_tokens)} output tokens`;
}

function generationStep(a: string, attempts: Attempt[]): Step {
  const title = `${a} generates`;
  if (attempts.length === 0)
    return { title, figure: "no record", text: `No generation attempt by ${a} is recorded.`, state: "missing", verdicts: [] };
  const byRoute = (r: Route) => attempts.filter((x) => route(x) === r);
  const parts = (["model", "fixture", "declared", "unrecorded"] as Route[])
    .filter((r) => byRoute(r).length > 0)
    .map((r) => `${byRoute(r).length} ${ROUTE_TEXT[r]}`);
  const packages = attempts.filter((x) => x.status.package).length;
  const model = byRoute("model");
  const fixtures = byRoute("fixture").length;
  return {
    title,
    figure: `${plural(attempts.length, "attempt")} · ${plural(packages, "package")}${fixtures ? " · fixture" : ""}`,
    text:
      `${a} recorded ${plural(attempts.length, "generation attempt")}: ${parts.join(", ")}; ` +
      `${packages} produced a package` +
      (model.length ? ` (model requests: ${cost(model)}, failures included).` : "."),
    state: "recorded",
    verdicts: [],
  };
}

/** The generation record of one revision: same organization and operation as
 * the revision observation, the exact request it names, and
 * when the record names a package, exactly the child. */
function revisionAttempt(l: Lineage, r: Extract<Edge, { kind: "revised_from" }>): Attempt | null {
  const matching = l.attempts.filter(
    (x) =>
      x.status.org_id === r.org_id &&
      x.status.operation_id === r.operation_id &&
      r.request_sha256 !== null &&
      x.status.request_sha256 === r.request_sha256 &&
      (x.status.package === null || refKey(x.status.package) === r.to),
  );
  return matching.length === 1 ? matching[0]! : null;
}

function revisionStep(l: Lineage, a: string, b: string, revisions: Extract<Edge, { kind: "revised_from" }>[]): Step {
  const title = `${b} revises`;
  if (revisions.length === 0)
    return { title, figure: "no record", text: `No revision by ${b} is recorded.`, state: "missing", verdicts: [] };
  const changed = revisions.some((r) => r.source_changed);
  const own = revisions.map((r) => revisionAttempt(l, r));
  const known = own.filter((x): x is Attempt => x !== null);
  let how: string;
  if (known.length < own.length) how = "no generation record with its exact request is supplied, so its cost is unknown";
  else if (known.every((x) => route(x) === "model")) how = `${cost(known)}; its own generation record`;
  else how = `its generation record shows ${known.map((x) => ROUTE_TEXT[route(x)]).join(", ")}`;
  return {
    title,
    figure: changed ? "source changed" : "source unchanged",
    text:
      `${b} recorded ${revisions.length === 1 ? "a revision" : `${revisions.length} revisions`} of ${a}'s method; ` +
      `the source bytes ${changed ? "changed" : "did not change"} (${how}).`,
    state: "recorded",
    verdicts: [],
  };
}

export function story(l: Lineage, a: string, b: string): Step[] {
  const generatedBy = (org: string) =>
    new Set(l.edges.flatMap((e) => (e.kind === "generated" && e.org_id === org ? [e.to] : [])));
  const aMethods = generatedBy(a);
  const revisions = l.edges.flatMap((e) => (e.kind === "revised_from" && e.org_id === b && aMethods.has(e.from) ? [e] : []));
  const bRevisions = new Set(revisions.map((r) => r.to));
  const decisions = l.edges.filter((e) => e.kind === "adoption" && e.org_id === a && bRevisions.has(e.to));
  const bTrials = trialsOf(l, b, aMethods);
  const bRevTrials = trialsOf(l, b, bRevisions);
  const aTrials = trialsOf(l, a, bRevisions);
  const trialStep = (title: string, who: string, trials: Trial[], what: string): Step =>
    trials.length
      ? {
          title,
          figure: `${trials.length} trials · ${tally(verdictWords(trials))}`,
          text: `${who} tested ${what} in ${trials.length} trial${trials.length === 1 ? "" : "s"} on ${families(trials)}${hiddenNote(trials)}: ${tally(verdictWords(trials))}.`,
          state: "recorded",
          verdicts: verdictWords(trials),
        }
      : { title, figure: "no record", text: `No record of ${who} testing ${what}.`, state: "missing", verdicts: [] };

  return [
    generationStep(a, l.attempts.filter((x) => x.status.org_id === a)),
    trialStep(`${b} tests it`, b, bTrials, `${a}'s method`),
    revisionStep(l, a, b, revisions),
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
