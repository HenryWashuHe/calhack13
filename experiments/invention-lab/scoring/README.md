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

- `accepted` when the candidate's successes are at least the control's plus
  `min_margin`;
- `rejected` when they aren't, or when the candidate's output is an invalid
  design;
- `insufficient` when any scoring is missing, failed or unknown. Missing
  evidence never counts as a pass or a rejection.

The verdict is acceptance evidence for adoption, not an adoption decision or
an installation.

