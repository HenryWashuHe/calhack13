# `process-comparison/1`: workflow versus fresh development (receiving side)

Status: **prepared, not executed.** This document is the receiving-side
declaration (trials, panels, ceilings, controls, selection, endpoints). It is
frozen by the commit that merges it. Codex prepares the six generation
operations and the full chain ceilings against it before any provider request.
No result exists yet. This is one exploratory pass, not G4.

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
  extra public input is the `lab-score feedback` from **all four** of round k's
  development runs (§ Development stage).
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

## Selection (frozen before the fresh panel)

For each arm and family, the selected method is the candidate with the most
development successes on that family. Ties are broken in order by:
1. fewer development simulation steps on that family;
2. earlier round;
3. lexically smaller package SHA-256.

A candidate whose development run on a family is `unknown`, `failed` or has no
output counts as 0 successes there and can be selected only if every candidate
does. If an arm produced no method at all, its selection is `no_candidate` for
every family.

## Fresh-panel stage

For each family, on seeds 1000..1011, with each slot run once in this order:
1. **Primary:** W-selected (candidate) vs F-selected (control).
2. W-selected vs seeded random search.
3. F-selected vs seeded random search.
4. Regression baseline: the unchanged parent `8215c367…` vs seeded random
   search.

A `no_candidate` side makes that plan `not_run`, recorded as such.

## Endpoints

- **Primary:** summed fresh-panel successes across the 4 × 12 instances,
  W-selected minus F-selected, from the primary plans. An unknown, failed or
  no-output slot counts 0 toward its sum and is listed.
- **Positive exploratory reading** only if W's sum exceeds F's by at least 2
  **and** W's sum is at least seeded random search's sum over the same
  instances. Otherwise the result is reported as null or negative.
- **Secondary:**
  - the per-family strict verdicts from all four plan types;
  - per-family regressions against the parent baseline;
  - the development-panel results of every candidate, including rejected
    ones.

Nothing here is a significance test. One K = 3 pass on 48 instances is
exploratory, says nothing about network effects and doesn't establish
adoption.

## Retained records

Every spec, snapshot, receipt, ledger, transcript, scoring record, verdict,
feedback document and summary is retained, including failures, `not_run`,
`no_parent`, `no_candidate` and unused capacity. Chain-level costs come from
`genesis-forge method-chain-cost-assess` over the declared chain. Per-trial
reports are never summed by hand.
