# PR #215: historical rehearsal archive review

Reviewed source: `15f6ccff26f1710a265695461977b57126fa5b19`, which includes
main `85073cc6208800e15b5cc96b742bfba07ef325b9`. Relative to that main, the
change contains three ignored rehearsal drivers and three evidence directories;
it changes no production implementation. This note and the restart correction
below are subsequent review changes, not inputs to the historical runs.

The evidence READMEs describe the dependency status **when those runs were
recorded**. Their statements that #65, #179, #185, #186 or #211 were unmerged,
or that result attachment was not yet implemented, are historical. Those
features and later fixes are now in main. The original READMEs, manifests and
records remain unchanged; merging this archive does not rerun it against the
current implementation.

## Retained bytes and missing files

All paths below are under `docs/invention-lab/evidence/`. Counts exclude the
manifest itself. Leading `./` in the first two manifests is normalized.

| Directory | Manifest entries | Present, matching SHA-256 | Missing |
| --- | ---: | ---: | ---: |
| `rehearsal-invocation-20261025` | 145 | 56 | 89 |
| `rehearsal-planner-chain-20261025` | 152 | 63 | 89 |
| `rehearsal-two-orgs-20261025` | 85 | 85 | 0 |

Every committed file other than `SHA256SUMS` has a matching manifest entry.
The missing files in the first two bundles are all under `artifacts/`, including
scoring metrics and submitted designs. Their manifests record bytes that are
not retained in Git. Treat them as partial historical archives, not complete
inputs for replay or independent acceptance recomputation. Do not regenerate
or silently remove those missing entries to make a manifest check pass.

The third bundle retains the scoring metrics and traces for both receiving
slots and the invocation, plus the invocation's submitted design. Its README
explicitly excludes exploration artifacts and SQLite databases. Thus its
complete manifest is still not a complete archive of every execution artifact.

## Data checks and their limits

A read-only JSON/hash audit checked all nine retained execution receipts against
their plans: slot identity, purpose, canonical plan and binding hashes, method,
environment, transcript hash and recorded usage limits. It checked all seven
retained scoring requests against their execution receipt and plan context,
including canonical request hashes and recorded scoring usage. Invocation
plans, server receipts and authorization grants agree in all three bundles.

For rehearsal 3, the retained metrics name the six ordered instances declared by
each plan. Their Boolean rows reproduce the recorded counts: candidate **6/6**,
control **2/6** on `45000..45005`, invocation **6/6** on `45100..45105`. The
attached result references the same evaluation and adoption/requirement/role/
responsibility as the invocation grant. The retained refusal records show 401
and 409, and A's final state and 404 run lookup match the stated control.

These are consistency checks of author-provided bytes. They do not authenticate
execution, reproduce the physics, prove process isolation, validate the current
namespace implementation, establish live model/provider activity, or demonstrate
recursive benefit. The planner is scripted and one operator controls all tokens.
No mechanism or evaluation goal gate closes from this archive review.

## Driver correction and compilation

The two-organization driver previously constructed B's replacement process
inside `mem::replace` before dropping the old process. Both could try to listen
on the same port. The review change drops and reaps the old B before spawning
the replacement with its receiver file. This fixes source ordering only; the
historical records were not altered or regenerated.

`cargo check --locked` passed for the explicit test targets
`genesis-planner::{rehearse_planner_chain,rehearse_two_orgs}` and
`genesis-server::rehearse_invocation` after this correction. All three remain
ignored by default. No rehearsal, generated method, evaluator, scorer,
namespace probe or live provider was executed in this review.
