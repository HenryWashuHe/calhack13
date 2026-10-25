# Invention-lab acceptance scoring (experimental)

Henry's Claude, issue #1 (assigned as the bounded extension of the evaluator /
executor lane). Standalone crate. It scores one slot that the executor already
ran and froze.

```sh
cd experiments/invention-lab/scoring
cargo test
cargo run -q --bin lab-score -- score <plan.json> <slot> <executor-state-dir> <score-dir>
# then, from ../executor: lab-exec check <plan.json> <slot> <state-dir> <score-dir>
cargo run -q --bin lab-score -- verdict <plan.json> <criterion.json> \
  <candidate-state> <candidate-score> <control-state> <control-score>
```

`lab-score` (`score_slot`) never runs a method, and nothing here is reachable by one.

1. It rebuilds the #22 `EvaluationRequest` from the frozen plan and the
   retained execution receipt (`TrialPlan::evaluation_request`).
2. It checks that the retained output bytes match the request's output
   digest, and that the linked evaluator matches the plan's.
3. It checks that the plan's separate scoring grant covers a full episode on
   every public development instance, before simulating anything.
4. It simulates every instance and writes the per-instance trace and a metrics
   artifact (`success_count`, `of`, per-instance success, progress and cost).
5. It returns a receipt that is validated against the rebuilt request.

Acceptance uses the plan's **public** development instances, the same ones the
method could experiment on, as #28 v1 specifies. Hidden comparison scoring and
the reveal aren't implemented.

## Acceptance verdict

`verdict` (module `verdict`) is the receiving organization's decision for a
candidate/control plan. It re-validates everything before deciding:

- the criterion bytes match `plan.criterion`, use rule `success-count-margin`,
  and list exactly the plan's public instances;
- both slots' execution and scoring receipts pass `assess_trials`;
- each metrics artifact matches its receipt and covers the criterion's
  instances.

Outcomes:

- `accepted` when both slots' development has a known outcome (a confirmed
  failure or interruption counts when its retained output was scored, as
  adoption.md specifies) with fully known resource accounting (or unknown only in fields the frozen criterion lists in
  `unknown_usage_allowed`), both scorings are `Scored` within their grants, and
  the candidate's successes are at least the control's plus `min_margin`;
- `rejected` when the margin isn't met, or when the candidate's output is an
  invalid design;
- `insufficient` for anything else: unknown or missing development, unknown usage the criterion doesn't allow, or missing, failed or
  unknown scoring. Missing evidence never counts as a pass or a rejection.

Metrics rows must be exactly one per criterion instance, in order, each naming
that instance and carrying a boolean `success`; nothing else is counted.

The executor records model use as unknown in every mode (it can't establish
zero), so under the default criterion a real sandboxed run is `insufficient`
unless the plan's criterion explicitly accepts unknown model calls and tokens.

The verdict is acceptance evidence for adoption, not an adoption decision or
an installation.


## Predeclared receiving runs (`tools/receive.py`)

`tools/receive.py <spec.json> <out-dir>` runs a whole set of receiving-side
trials from one `lab-receive-spec/1` file, with `LAB_EXEC` and `LAB_SCORE`
pointing at the built binaries. The spec names the receiving organization,
the sandbox, equal per-arm grants, each method package by path and expected
SHA-256, and the trials (family, candidate, control). The spec and every
package are read once, checked, and retained under the output directory
(`spec.json`, `packages/`). Only those snapshots are used afterwards, so editing
the inputs mid-run changes nothing. Publish the spec or its hash before the run
starts. The hash the tool prints isn't proof of a prior declaration.
Each trial is a fresh two-slot plan from `lab-exec example-plan`, with the
control method, slot organization and plan id rewritten. Each slot runs once,
then the tool scores both, decides the strict verdict and writes the
candidate's `lab-score feedback`. Failures are recorded, not retried.
`summary.json` is rewritten after every trial. An unexpected error records an
error row and stops the run. The output directory must not already exist, and
the summary repeats the spec hash and the host's runtime ref. A package whose bytes don't match its
declared hash stops the run before anything executes.
