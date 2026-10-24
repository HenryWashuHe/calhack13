# PR review queue — October 22, 2026

Reviewed main checkpoint: `a313a8e151f2110847d2bb93651d127eab1f34ca`.
The user authorizes merging reviewed ready PRs. Preserve ancestry and check the
exact resulting tree; do not merge a passing but outdated candidate after its
source changes. Current GitHub heads and comments supersede this snapshot.

## Landed milestones

| PR | Main merge | Reviewed source and evidence |
| --- | --- | --- |
| [#20 specialist execution](https://github.com/HenryWashuHe/genesis/pull/20) | `38aa622` | `5798cb4`, exact tree. Earlier integration: 620 Rust tests; final 53 affected tests, strict Clippy, 17 web tests/build and schema checks. Includes #36/#41. |
| [#35 venue portal](https://github.com/HenryWashuHe/genesis/pull/35) | `f51d4d9` | Computed merge tree matches; 16 portal tests, no skips, independent fetched A/B pages verified. Includes #40 path collision fix. |
| [#12 planner](https://github.com/HenryWashuHe/genesis/pull/12) | `90433a8` | `e5835c8`, exact tree; runtime `0cad0c4`: 649 Rust tests, strict default Clippy, 17 web tests/build and schema checks. Final delta was docs only. Includes #38/#44. |
| [#11 Elastic](https://github.com/HenryWashuHe/genesis/pull/11) | `8c1d292` | `4c5181f`, exact tree; 260 focused tests, strict default workspace Clippy, 21 web tests/build and schema checks. Includes #39/#46. |
| [#22 Elastic planner](https://github.com/HenryWashuHe/genesis/pull/22) | `09c13ee` | `54d0d2b`, exact tree; 57 planner/Elastic tests and strict default workspace Clippy. Web/schema unchanged. Includes #42/#48. |
| [#49 capability gaps](https://github.com/HenryWashuHe/genesis/pull/49) | `08dfece` | Corrected `c54d339`: 83 core/planner/server tests, 21 web tests/build, schema/type checks. Clean integration `579e31c`: 58 planner/Elastic tests and strict Clippy; main tree matches. |
| [#21 evidence UI](https://github.com/HenryWashuHe/genesis/pull/21) | `f8856f1` | `84463a1`, exact tree; 40 tests, clean install, typecheck/build. Includes reviewed #45 and integrations #53/#55, preserving both authors' assertions. Backend/schema unchanged. |
| [#54 gap lifecycle UI](https://github.com/HenryWashuHe/genesis/pull/54) | `50db34b` | `e212741` clean integration, exact computed merge tree; 42 web tests/typecheck/build. Backend/schema unchanged. Duplicate #50 closed with branches preserved. |
| [#23 development guide](https://github.com/HenryWashuHe/genesis/pull/23) | `a313a8e` | Reviewed child #58 `ee37037` / parent `99db48a`; final DEV.md-only delta, 12 relative links and whitespace checked, exact computed merge tree. |

Rust functional checks above use `serde_json/preserve_order`; strict Clippy uses
default features. An additional planner integration Clippy run with the JSON-order
feature reported size lints and is not claimed green. Counts belong to their
listed sources, not an aggregate current-main test run. Review caches are separate
per checkout; the specialist/planner cache is exclusively
`/tmp/genesis-pr20-review-target`, while other checkouts use their own `target`.

The earlier capability stack #8–#10/#13–#19/#24 and CREAO execution receipts #32
are on main. Historical detailed evidence remains in
[the prior queue](https://github.com/HenryWashuHe/genesis/blob/a857a9f/docs/PR_REVIEW_QUEUE.md).

## Holds and active reviews

| PR / inspected source | Remaining work |
| --- | --- |
| [#26](https://github.com/HenryWashuHe/genesis/pull/26) `8c424ce`, [#37](https://github.com/HenryWashuHe/genesis/pull/37) `5e6072d` | Durable provisional-receipt correlation: original failed-retirement regression and real reopen variant fail. [Current review](https://github.com/HenryWashuHe/genesis/pull/26#pullrequestreview-5619948320), test-only `fed49ff`. The earlier alternative `51dd431` had a first trusted-reference-save failure, preserved at `f939615`. |
| [#27](https://github.com/HenryWashuHe/genesis/pull/27) `29c9853`, [#28](https://github.com/HenryWashuHe/genesis/pull/28) `606b92b`, draft #29 | Current #27 passes 23 focused tests and strict server Clippy ([review](https://github.com/HenryWashuHe/genesis/pull/27#issuecomment-6350033955)); #28 passes 32 web tests/typecheck/build ([review](https://github.com/HenryWashuHe/genesis/pull/28#issuecomment-6350024085)). Their own changes are conditionally ready; readiness ancestry and final integration remain pending. No native readiness or skill-installation proof. |
| [#47 live gates](https://github.com/HenryWashuHe/genesis/pull/47) `0aa2169` | Original four regressions pass. Follow-up `7be7164` reproduces CREAO quota history resetting per run and unusable resume commands. [Review](https://github.com/HenryWashuHe/genesis/pull/47#issuecomment-6349759998). |
| [#51 runbook](https://github.com/HenryWashuHe/genesis/pull/51) `c354bb8` | Correct clean-checkout build/PATH commands, provider-access acknowledgment and example/verification overlap wording. [Review](https://github.com/HenryWashuHe/genesis/pull/51#issuecomment-6349760305). |
| [#52 planner eval](https://github.com/HenryWashuHe/genesis/pull/52) `4a7a261` | Model errors/fallback escalations count as successful budget decisions; assigning the wrong existing role passes the capacity scenario. Three failing regressions at `1b14990`; existing eight tests and strict crate Clippy pass. [Review](https://github.com/HenryWashuHe/genesis/pull/52#pullrequestreview-5620023859). No live model run. |
| [#57 gap planner wakeup](https://github.com/HenryWashuHe/genesis/pull/57) `86d8168` | Terminal gap-transition behavior and recovery under review. |
| [Draft #56 gap policy](https://github.com/HenryWashuHe/genesis/pull/56) `cf591a3` | Tested policy materializer; server worker, durable gap binding and explicit startup opt-in remain Codex's next core slice. |

Draft [#25](https://github.com/HenryWashuHe/genesis/pull/25) is pushed at `2829025`
with 132 affected Rust tests and strict default Clippy. It includes main through
#22 but not #49/#21/#54; it must not merge wholesale while readiness findings remain.
See [the combined checkpoint](COMBINED_INTEGRATION.md).

The mission still needs planner-driven acquisition, verified publication and
independent adoption in the visible workflow, persistent CREAO core mission work,
and native sponsor receipts. Fixture execution, API presence, callback reachability
and a green PR do not establish those live outcomes. Coordinate ownership and
merge sequencing in [issue #4](https://github.com/HenryWashuHe/genesis/issues/4).
