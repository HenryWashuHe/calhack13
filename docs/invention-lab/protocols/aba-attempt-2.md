# `aba-attempt-2`: B revises, A independently decides (receiving side)

Status: **proposed, not frozen, nothing run.** This is Henry's Claude's
receiving-side declaration for the next A → B → A mechanism attempt agreed in
principle in
[issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1).
Codex owns the generation side: the exact-parent revision input, the one
generation attempt, its model, settings and output allowance, and the chain
allocation. Codex adds those to this declaration before anything is frozen.
The attempt is exploratory. It isn't the controlled G4 comparison, and it
doesn't need a positive result to be reported honestly. A decline is a
result.

## Organizations, roles and responsibilities

- **A (`org_example_a`):** publisher of the parent `8215c367…` and the A return
  receiver. A's intended owner is **its existing design role**: the role that
  already owns A's ravine-crossing design responsibility before the return
  observation. This is the existing-owner reuse case. A's planner must assign
  the return requirement to that role, not create a specialist. If it
  proposes a specialist instead, that proposal is recorded as the outcome and
  A does not continue to adoption under a different owner.
- **B (`org_example_b`):** holds the child `4f455dc6…` it accepted earlier
  (#116 against the fixed cart). B revises the child and receives its own
  revision.
- A and B run as separate organization servers with separate databases,
  operator tokens, receiver configurations and records roots. Each
  organization's decisions are recorded only in its own store.

## Stages

1. **B revision (Codex).** One declared generation attempt, with exact parent
   the child `4f455dc6…`. The revision input is B's full public feedback from
   its #116 trials, **including the negative ones** (child vs random search
   3/6 vs 4/6 Rejected, child vs parent 3/6 vs 6/6 Rejected, child vs fixed
   cart Accepted). Separately hashed and labeled as a **public human
   intervention**, not B-generated feedback, it also includes a short summary
   of `analysis/family-robustness` (#184): the fixed designs per family, the
   observation that the parent's output doesn't depend on the family, and
   the headroom table. That summary is author analysis, not evidence that any
   family can't be improved. No hidden panel and no retry. The output is
   frozen as `child2` before stage 2.
2. **B receiving trial.** `receive.py`, spec `b-receive.json` (template
   below). `child2` vs B's pinned control, the fixed cart `dc5b74f3…`.
   Rough-transport, fresh public seeds **43000..43011** (12 instances),
   margin one. Reported-only trials on the same panel: `child2` vs random
   search `06777e8e…` and vs the child `4f455dc6…`. B publishes `child2` to
   the registry only if its pinned trial accepts. Otherwise the attempt ends
   at B with a decline.
3. **A selection and responsibility.** A finds `child2` by registry search
   and posts it to `/missions/{m}/method-development` with the need "develop
   ravine-crossing designs" and the acceptance below. A's planner decides
   ownership (see A's intended owner above).
4. **A receiving trial (fresh return task).** Spec `a-receive.json`. `child2`
   vs **A's own current method, the parent `8215c367…`** (existing-owner
   adoption control). Ravine-crossing, fresh public seeds **44000..44011**
   (12 instances), margin one. Reported only: `child2` vs random search on
   the same panel. A's receiver pins exactly this trial's plan for the return
   requirement before the trial runs.
5. **A decision.** A's owner adopts `child2` if the pinned trial is Accepted
   and declines otherwise. The decision is recorded by A's server through
   `/method-adoptions`.
6. **A execution under the grant.** Only if adopted: one invocation run
   (`invoke.py`, #186) on fresh ravine seeds **44100..44111**, same grants.
   Its output is scored descriptively with `lab-eval accept` on those
   instances. This is not an acceptance or a comparison.

## Fixed parameters

| | B receiving | A receiving | A invocation |
| --- | --- | --- | --- |
| family | rough-transport | ravine-crossing | ravine-crossing |
| seeds | 43000..43011 | 44000..44011 | 44100..44111 |
| pinned control | fixed cart `dc5b74f3…` | parent `8215c367…` | — |
| reported only | random `06777e8e…`, child `4f455dc6…` | random `06777e8e…` | — |
| development grant | 72,000 steps, 60,000 ms, zero model allowances | same | same |
| scoring grant | 600,000 ms, 12 × 2,400 steps | same | — |
| criterion | margin one | margin one | — |
| sandbox | namespaces, pinned Python 3.14.0 runtime ref `0b21e795…` | same | same |

Spec templates, filled in at freeze; `child2`'s path and sha256 come from the
generation record:

- `b-receive.json`: org `org_example_b`, 12 instances, packages `child2`,
  `fixed-cart`, `random-search`, `child`; trials `b2-pinned` (child2 vs
  fixed-cart), `b2-vs-random`, `b2-vs-child`, all `first_seed` 43000.
- `a-receive.json`: org `org_example_a`, 12 instances, packages `child2`,
  `parent`, `random-search`; trials `a2-pinned` (child2 vs parent) and
  `a2-vs-random`, both `first_seed` 44000.

## Readings

- **G3-relevant outcome:** A's pinned trial Accepts, A's owner adopts, the
  invocation completes under A's grant, **and `child2` does not lose to random
  search on A's panel** (`a2-vs-random`: `child2`'s success count at least
  random search's). Even then this is one exploratory chain.
- **Adopted, not G3-relevant:** A's pinned trial Accepts and A adopts, but
  random search beats `child2` on the same panel. The adoption stands as A's
  own decision against its own method; it is reported as reuse of a method
  better than A's, not as improved development. Rehearsal 5 ended this way
  with the stand-in (3/12 vs parent 2/12, random 6/12).
- The random comparison stays out of A's receiver pin: the receiver decides
  against what A has, and this floor only governs how the result is read. It
  is fixed here before freeze so it can't be chosen after seeing the numbers.
- **Decline:** any pinned trial rejects. Everything is retained and
  published the same way.
- Failed, interrupted or unknown executions are ineligible, as in
  `process-comparison/1`.

## Prerequisites before freeze

- #57/#65 namespace assurance reviewed (runtime evidence for stages 2, 4 and
  6).
- Codex's generation declaration added here (stage 1).
- The two organizations' configurations, tokens and receiver pins written
  down in this directory before stage 2.
