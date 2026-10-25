# `process-comparison/1`: workflow versus fresh development (receiving side)

Status: **prepared, not executed.** This document is the receiving-side
declaration (trials, panels, ceilings, controls, selection, endpoints). It is
frozen by the commit that merges it. No result exists yet. This is one
exploratory pass, not G4.

## Preparation and chronology

Before any provider request, the chain declares **six operation slots** (W1,
W2, W3, F1, F2, F3) with their full per-call ceilings and whole-chain ceiling.
Only the slots and ceilings are fixed up front:

- W1, F1, F2 and F3 depend on nothing produced here, so their exact Prepared
  requests may be bound before the first request.
- W2's and W3's exact Prepared requests depend on feedback that doesn't exist
  yet. Each is bound **after** its declared feedback (§ Development stage)
  exists and **before** that request is sent. No future request hash is
  written in advance.
- A slot that never gets a Prepared request (`no_parent`, below) keeps its
  status and its full unused capacity in the retained records. Its cost stays
  unresolved; no zero-valued completed record is invented for it.
- Capacity is never allocated retrospectively.

## Question

Does a development **workflow** that revises its own methods from checked public
feedback (W) produce better methods than the same budget spent on **independent
fresh generations** (F)?

## Arms

Both arms use the same exact model and settings: `claude-sonnet-5-5`, with the
explicit adaptive/high reasoning policy from #120. They also share the same
per-call caps, base task description, protocol, design-language document and
runtime/protocol references.

- **W:** three rounds. Round 1 is one fresh generation from the base plan.
  Round k+1 (k = 1, 2) is one exact-parent revision of round k's method. Its
  extra public input is the `lab-score feedback` that round k's four
  development runs produce (§ Development stage defines refusals).
- **F:** three independent fresh generations from the same base plan, with no
  feedback and no W history.
- **Neither arm** receives the earlier #116/#117 feedback or any earlier
  generated method.
- K = 3 attempts per arm. A failed or unknown generation consumes its attempt
  and produces no method; it is never replayed. If W's round k yields no
  method, rounds after it have no parent and are recorded as `no_parent`. They
  are not replaced by fresh generations.

## Families and panels

All four families: ravine-crossing, rough-transport, return-mix and
projectile-target, family version 1.

- **Development panel:** seeds 0..5 per family. Only this panel's records may
  feed W's revisions or either arm's selection.
- **Fresh public panel:** seeds 1000..1011 per family, built with
  `lab-exec example-plan … <first-seed>` (#121). These instances are given to
  the method as public development inputs *during the final run*. This is a
  fresh-task test of a frozen method on a previously unused panel. It is
  **not** hidden or held-out design scoring, and it doesn't prove the panel was
  never visible to anyone. Genuinely hidden scoring stays with the separately
  reviewed #57 path. No method, prompt, selection or revision may use any
  fresh-panel result. The fresh-panel stage starts only after both arms'
  selections are frozen and posted.

## Execution ceilings and environment

Every method run has the same ceilings:
- 72000 simulation steps, 60000 ms and 0 model calls per run.
- Scoring: instances × family max steps (2400, or 1200 for projectile-target)
  and 600000 ms.

All runs use the `namespaces` sandbox, a declared `python3` reporting exactly
the runtime the packages pin, and the executor/scorer at one recorded commit.
Every stage runs through `scoring/tools/receive.py` (#118). Each spec's SHA-256
is posted on issue #1 before that stage's invocation. Grants and actual costs
are reported separately: equal maximum capacity is not equal realized spend.

## Development stage (per candidate)

Each produced method runs once on each family's development panel. It is the
candidate in a two-slot plan whose control is the seeded hand-written
random-search package (`06777e8e…`), under the strict criterion
(`success-count-margin`, margin 1, no unknown usage). Order is fixed:
1. arm F before arm W;
2. within an arm, round order;
3. within a candidate, families in the order listed above.

W's round-k development runs finish, and their feedback is produced, before
round k+1 is requested.

`lab-score feedback` refuses an unscored run (no retained output, scoring not
`Scored`). So round k+1's feedback input is exactly the set of feedback
documents that `lab-score feedback` produces for round k's four candidate
slots, in the family order above; each refused family is listed as absent with
its refusal. If all four are refused, round k+1 and every later W round are
recorded as `no_feedback`, are not requested, and keep their unused capacity
(same retention rule as `no_parent`). If the revision contract can't carry the
produced documents as checked public inputs, the run stops before W2 and the
protocol is amended; nothing is improvised mid-run.

## Selection (frozen before the fresh panel)

For each arm and family, candidates are split by their development run on
that family:

- **Scored:** the candidate slot's scoring receipt is `Scored` and its success
  count is known from the retained metrics.
- **Unscored:** anything else (generation produced no method, execution
  `failed` or `unknown`, no output, scoring not `Scored`, or an unknown
  success count).

Only scored candidates are eligible. A valid score of 0 is eligible; an
unscored run is never treated as 0. Among eligible candidates, the selection is
the one with the most development successes. Ties are broken in order by:
1. fewer development simulation steps on that family, where an unknown step
   count ranks after every known one;
2. earlier round;
3. lexically smaller package SHA-256.

If every candidate of an arm is unscored on a family (including an arm that
produced no method at all), that arm's selection for that family is
`no_candidate`. Unscored runs stay listed with their status. Nothing is rerun
to improve eligibility.

## Fresh-panel stage

For each family, on seeds 1000..1011, with each slot run once in this order:
1. **Primary:** W-selected (candidate) vs F-selected (control).
2. W-selected vs seeded random search.
3. F-selected vs seeded random search.
4. Regression baseline: the unchanged parent `8215c367…` vs seeded random
   search.

A `no_candidate` side makes that plan `not_run`, recorded as such. The
regression-baseline plan (4) never depends on a selection and is always run.

## Endpoints

- **Primary:** summed fresh-panel successes across the 4 × 12 instances,
  W-selected minus F-selected, from the primary plans (1) only.
  - If any family has no primary plan (`not_run` because either side is
    `no_candidate`), the primary comparison is reported **incomplete / not
    estimable**. Available per-family primary and companion results are still
    reported, but no positive reading is allowed and no secondary run is
    substituted for a missing pair.
  - Within a primary plan that ran, an unknown, failed or no-output slot is
    listed and contributes 0 to its arm's sum for reporting. It also makes the
    positive reading unavailable (condition 2 below).
- **Canonical random reference:** the control slots of the four
  regression-baseline plans (4), parent vs seeded random search. "Random
  search's sum" means only those slots' fresh-panel successes. Random runs in
  plans (2) and (3) are reported separately as repetitions and never replace
  the reference.
- **Positive exploratory reading** only if all of these hold:
  1. the primary comparison is complete (all four primary plans ran);
  2. every primary slot was scored (no unknown, failed or no-output slot);
  3. all four reference slots were scored;
  4. W's sum exceeds F's by at least 2;
  5. W's sum is at least the reference sum.

  A missing or unknown reference observation makes the positive reading
  unavailable; it is never read as a zero baseline. Otherwise the result is
  reported as null, negative or incomplete, whichever applies.
- **Secondary:**
  - the per-family strict verdicts from all four plan types;
  - per-family regressions against the parent baseline;
  - the random repetitions from plans (2) and (3);
  - the development-panel results of every candidate, including rejected
    ones.

Nothing here is a significance test. One K = 3 pass on 48 instances is
exploratory, says nothing about network effects and doesn't establish
adoption.

## Retained records

Every spec, snapshot, receipt, ledger, transcript, scoring record, verdict,
feedback document and summary is retained, including failures, feedback
refusals, `not_run`, `no_parent`, `no_feedback`, `no_candidate` and unused
capacity. Chain-level costs come from
`genesis-forge method-chain-cost-assess` over the declared chain. Per-trial
reports are never summed by hand.
