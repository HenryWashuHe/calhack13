# PR review queue — October 23, 2026

## Current queue after the scenario repair

Main through #111 is `6194e53`. Current GitHub status and [issue #4](https://github.com/HenryWashuHe/genesis/issues/4)
supersede the historical tables below.

| PR | Disposition / source | Evidence |
| --- | --- | --- |
| #86 | Merged `ad622a9`, reviewed `ac50d35` | 35 gap-adoption tests and two-org inheritance pass; retained native receipt failures remain blocked. Strict Clippy/fmt. |
| #111 | Merged `6194e53`, reviewed `84dd81f` / composition `e1b95ea` | Reproduced full-scenario deadline failure, then five scenarios and 40 scenario units pass. Runtime freshness remains strict; Claude contribution preserved. |
| #112 | Implemented `1b2610c` + tests `bbe1872`; current merge status on GitHub | 139 library + 90 assessment tests, then final 12 intake tests; strict Clippy/fmt and independent review. Loopback evidence; counts overlap. |
| #110 | **Hold** at `91b0169`; supersedes #91 | Structural updates every second still starve replay. Test-only `b677414`: one reproduced failure plus three unchanged passing original controls. |

Claude owns #110's production fix and the separately agreed terminal-outcome
retry follow-up. Codex owns automatic recovery intake and its native/scenario
composition. Public Agentverse/ASI and live provider gates remain unproven.

## Historical queue after #108

Main `74b99c3fabf6d17188b5bf53fba9cd83d828305d`, tree
`f75934c42770f902e40700079b50d9972806d220`. Newer GitHub heads and issue #4
supersede this snapshot; preserve the original reproduced tests before merging.

| PR | Current disposition | Evidence |
| --- | --- | --- |
| #84 | Merged `d46b03f`, source `c7d2ee9` | Four preflight tests and strict Clippy; docs accurately keep #86 retry unreleased. |
| #103 | Merged `743c9d1`, source `de98523` | 83 focused tests, strict server/planner Clippy; adoption-only names do not acquire broader policy scope. |
| #104 | Merged `a0b1ff6`, source `8a39f18` | 86 web tests, typecheck, build. |
| #105 | Merged `8e046c7`, source `dcc3648` | Explicit native Fetch/coordinator test passed; Apify/CREAO remain fixtures. |
| #106 | Merged `8f86571`, source `be49d82` | Additive Claude coverage: 66 venue and 77 Python tests; original native gate unchanged. |
| #107 | Merged `2ad34dd`, source `6676c52` | 104 Python tests before narrow constructor adjustment; final 27 ACP tests pass. No public lifecycle/ASI proof. |
| #108 | Merged `74b99c3`, source `180d37c` | 135 server-library + 81 assessment tests, strict Clippy/fmt. Baseline two failures plus one provider-absence control; stopped intents only retire after definitive child absence. |
| #86 | **Hold**, head `c2dc523483b3d474cd52e5978f2ad22dc486f93f` | HTTP401 run-status read → local Failed → retry duplicates a still-RUNNING effect. Test-only `d84eb44`, fresh composition: 33 pass, 1 fail. |
| #91 | **Hold**, head `6776130365fb04ba0c77b589ba82688c9e553edc` | Frequent events starve replay; absent revisions appear audited. Original `42aab93` regression branch is preserved. |

See [the current handoff](AGENT_HANDOFF.md) for ownership and next automatic
recovery intake work. Local validation does not establish cloud-provider success.

## Historical queue after #101

Main `0a0c175aa452d2dfcdee617e6f1647a523f5cceb`, tree
`ec382ccbfabc37487d23e3962e955f49f295493c`, includes #101 and the #83/#85/#87
integration/rehearsal stack. Current GitHub
heads and [issue #4](https://github.com/HenryWashuHe/genesis/issues/4) supersede
this snapshot. The user authorizes ready merges; the active root reviewer
sequences them after exact-source review and validation. The mission goal is
still active, with no native sponsor outcome established.

### Merged in this review batch

| PR | Merge / reviewed source | Evidence and scope |
| --- | --- | --- |
| [#78 finalizer](https://github.com/HenryWashuHe/genesis/pull/78) | `f4b5cdf` / `f76a1ba` | Earlier requirement, acceptance-suite, authority and full-manifest conflict holds resolved before merge. Opt-in verified acquisition → publication/local acceptance is now on main. |
| [#80 finalizer UI](https://github.com/HenryWashuHe/genesis/pull/80) | `d119341` / `0193143` | Original no-record regression and lifecycle controls pass; global enablement no longer claims every acquired gap is queued. Web tests, typecheck and build passed in reviewed composition. |
| [#88 settled-check display](https://github.com/HenryWashuHe/genesis/pull/88) | `62edcee` / `0c2eb75` | Settled check jobs stop showing misleading unproven work; genuine unknown jobs remain visible. Web tests/typecheck/build passed. |
| [#89 planner policy context](https://github.com/HenryWashuHe/genesis/pull/89) | `fd9d161` / `ae6d0e8` | Planner receives the configured capability names, source URLs and required scope. Model behavior still needs separate evidence. |
| [#97 console guards](https://github.com/HenryWashuHe/genesis/pull/97) | `8cd0e37` / `205ffed` | Test-only guards; meaningful positive and rejection assertions, no weakened existing tests. |
| #90 / #92 / #93 / #94 / #95 / #96 | Final merge `97d75eb`, exact composition tree `f305ca60` | **608 Rust tests / 68 targets**, scoped strict Clippy and fmt. Includes saga crash/refusal coverage and fixes, independent audit connection, validation/invariant controls and planner transport guards. Fake providers/runtime teardown do not establish native provider recovery. |
| [#98 Fetch float identity](https://github.com/HenryWashuHe/genesis/pull/98) | `9f9a87d` / `11703e2` | **133 Fetch/verification tests**, including ten float cases. Three baseline failures and two stale-intent release regressions reproduced. Exact float identity is preserved; stale drift cannot block release or healthy independent work. |
| [#99 monitor intake](https://github.com/HenryWashuHe/genesis/pull/99) | `72cd014` / `05d55e2` | **34 assessment + 117 server-library tests**, scoped strict Clippy/fmt. Four baseline failures and four controls. Persisted newest-candidate scan, manual-consumption handling and temporary provider waits retain child recovery and authority fencing. |
| [#82 adopt first](https://github.com/HenryWashuHe/genesis/pull/82) | `9a9c7a0` / correction `ae00fa9`, tested composition `4c8c461` | **41 focused tests**, scoped strict Clippy/fmt. Original same-owner delegation regression passes unchanged. Main tree `05767561` matches the tested composition. Author branch `henry-claude/gap-adoption` was fast-forwarded to that composition without rewriting history; Claude should pull before continuing it. |
| [#83 two-org integration](https://github.com/HenryWashuHe/genesis/pull/83) | `b58ac99` / `7c1900d` | Two-organization integration passes, including another run with #101. Real workers execute after seeded proposals; B runs its own checks without synthesis. Fixed external receipts do not prove unseen-layout adaptation. |
| [#85 adoption UI](https://github.com/HenryWashuHe/genesis/pull/85) | `090700b` / `c2abcf9` | **85 web tests**, typecheck and build pass in the integrated composition. Existing lifecycle controls are preserved. |
| [#87 rehearsal](https://github.com/HenryWashuHe/genesis/pull/87) | `8027428` / `88d9174` | Local rehearsal reached A synthesis/publication/installation, then B discovery/checks/installation with no B synthesis. Explicit fixture mode seeds proposals; optional external planner mode is separate. Every external build/run reply remains a fixture. |
| [#101 Fetch venue recovery](https://github.com/HenryWashuHe/genesis/pull/101) | `0a0c175` / `093010b`, exact tree `ec382ccb` | **255 focused Rust tests** (42 assessment + 135 server-library + 78 Fetch), the newly merged two-org test, **34 Python tests**, scoped strict Clippy/fmt and clean independent source review. Operator-started recovery preserves verified source/child identity and retires local grants with bounded exact cleanup. Queue acknowledgment is not peer release. The Python signed-process and coordinator loopback paths were tested separately. |

These counts belong to the stated source/composition, not an aggregate test run
of the final main tree. Earlier #81 immutable-receipt integration is also merged
as `0c689d5`; #98 updates its JSON identity behavior. All provider evidence here
is loopback, synthetic authenticated-boundary or local process evidence.

### Open or dependent

| PR / exact reviewed head | Status and next step |
| --- | --- |
| [#86 adoption retry](https://github.com/HenryWashuHe/genesis/pull/86), `83998b051ad95e13310db52e57857aa9ecf386d8` | **Hold: new failure reproduced.** Suite drift can make a local check `Rejected` while its native receipt still refers to a `RUNNING` provider operation. Retry submits POST 2 while a separate GET confirms the original run remains `RUNNING`. Test-only `a497651` on `codex/review-adoption86-rejected`, composed with then-current main as `c33c93f`, reproduces this (one failing regression). Old `0ec6606` also had two reproduced unknown-child retry failures; resolving those alone does not clear this new hold. |
| [#91 provenance console](https://github.com/HenryWashuHe/genesis/pull/91), `6776130365fb04ba0c77b589ba82688c9e553edc` | **Hold.** Nonstructural polling can continually reset replay; absent revisions are marked as audited success. Test-only `42aab93` has two failing regressions and a passing idle control. Original 125 web tests/typecheck/build passed, so those alone do not clear the hold. |
| [#84 runbook/preflight](https://github.com/HenryWashuHe/genesis/pull/84), `3d3219c93bbb065d794bfb3a68afba455e1340e1` | **Draft documentation hold.** Four preflight tests and scoped strict Clippy pass; the original policy-validation defect is cleared. The remaining hold is `docs/DEMO_RUNBOOK.md:311`: it presents unreleased #86 retry behavior and claims a check is never submitted twice, despite the reproduced #86 duplicate-run path. Remove or qualify that claim before merge. |

#83/#85/#87 merged independently of #86; the held retry is not required for
those completed milestones. Preserve source branches and exact-head review
regressions; never merge an outdated tested head after the source changes.

Claude/Eric owns kernel/planner/readiness/adoption/UI and their open fixes.
Claude's [requestable-adoption claim](https://github.com/HenryWashuHe/genesis/issues/4#issuecomment-6372394586)
reserves `crates/genesis-server/src/routes/capability_gaps.rs`,
`crates/genesis-planner/src/prompt.rs`, matching `RequestPolicy` / `Requestable`
deserialization and adjacent tests for adoption-only policy visibility. It does
not change the coordinator, kernel or acquisition. Retarget the dependent PRs
to current main as needed; do not delete author branches to clear stale bases.
Codex's #98/#99 corrections and #101 venue recovery are merged. Its next focus
is a composed fixture-backed integration of the native Python bridge and Rust
recovery coordinator; the separately passing paths do not yet establish that
combined execution. See [the recovery contract](VENUE_RECOVERY.md).
Credentials are not yet supplied; native Apify, CREAO, Elastic and Fetch gates
and genuine CREAO core-work/Share-link evidence remain open. See
[the handoff](AGENT_HANDOFF.md) and [live gate setup](LIVE_GATES.md).

## Historical review checkpoints

Everything below is preserved history. Old holds, queue-closeout statements and
“next implementation” assignments do not supersede the authoritative queue above.

## Historical review batch — October 22, 2026

- Immutable Fetch receipts (`afbe0bf`, original `6424bfb`): 64 crate tests including 13 new receipt checks, strict crate all-target Clippy, workspace fmt and clean independent review. Exact expected messages plus prepared/Applied journal chain; stable historical hashes, private serialization-only types and bounded/redacted payloads. Synthetic authenticated-boundary fixtures, no SDK/cloud/domain-verification claim. Server integration remains future work.

- [#79 subscribed monitoring scenario](https://github.com/HenryWashuHe/genesis/pull/79): **merged** as `1fc1b8e`, reviewed source `135c839`, exact tree `c09245b`. Runtime `8741f38`: 40 scenario unit/HTTP checks and five full scenarios, strict default workspace all-target Clippy/fmt, independent review clean. One durable registration and exact second-proof replacement requirement; loopback evidence only.
- [#78 gap finalizer](https://github.com/HenryWashuHe/genesis/pull/78): **hold** at `7cb747c`. Review composition `6be7dc2` on `codex/review-gap-finalizer78` has 11 passing controls and five reproduced failures: stale requirement installation/dispatch, weaker-suite acceptance reuse, Accepted shortcut after real delegation, conflicting same-pin publication. Detailed review and scoped fix handoff are on GitHub. Claude owns runtime fixes; review branch changes only tests plus main composition.
- [#80 finalizer UI](https://github.com/HenryWashuHe/genesis/pull/80): **hold** at `931192c`, stacked on #78. All 66 original web tests/typecheck/build pass. Test-only `d369213` reproduces a false queued claim for global enablement with no matching per-gap rule; organization-switch and terminal AttentionPanel controls pass. Await corrected parent and wording/eligibility.

- [#77 continuing venue subscriptions](https://github.com/HenryWashuHe/genesis/pull/77): **merged** as `1bb3c9f`, reviewed source `c889491`, exact tree `3ad4418`. Runtime `e699659` was based on main `3ea5ec4`. 154 source-scoped Rust checks: 113 server library, 14 CLI, 22 assessment integration including nine subscription checks, and five full scenarios. The final test-only addition passed with all nine subscription tests; earlier 21 assessment tests passed together. Strict default workspace all-target Clippy/fmt pass. Genuine Rust clients/private journals over loopback, trusted fixture publication/provisioning; no native sponsor claim. Crash-link recovery, authority/unknown/stop bounds and 205-event late-verification regression pass. Independent runtime review is clean through `e5ca93d`; later handoff edits are documentation only.

- [#76 immutable monitoring setup](https://github.com/HenryWashuHe/genesis/pull/76): **merged** as `3ea5ec4`, reviewed source `1ff0c94`, exact tree `fe419e7`. Implementation `bfcfe00` was based on main `64d6492`. All 35 portal tests pass without skips, including real Rust verifier and strict scenario CLI. Independent review reproduced and fixed public symlink aliases and non-string generator identities. Exact documented walkthrough also passes in a fresh temporary directory. Offline configured snapshot coverage only.

- [#75 continuing CREAO scenario](https://github.com/HenryWashuHe/genesis/pull/75): **merged** as `7e92e37`, reviewed source `67291ab`, exact tree `a9e63cd`. Runtime `f5136fe` based on reviewed main `8187430`. 145 default-feature Rust checks across four targets pass: 113 server library (37 scenario unit/HTTP), 14 CLI, 13 assessment and five full scenario tests. Strict default workspace all-target Clippy/fmt pass. Three observer regressions failed before their fixes. Real CREAO creation/runtime/journal against loopback, explicitly labeled fixture readiness; no live sponsor or Share-link claim.
- [#72 persistent CREAO assessments](https://github.com/HenryWashuHe/genesis/pull/72): **merged** as `8823d81`, exact reviewed source tree `50b508a`; runtime `0dc0986`. Durable specialist work over verified invocation facts, independent semantic acceptance, atomic replacement requirements and queued-effect authority checks. `1a340a9` passed 858 Rust tests / 90 nonempty suites (109 including empty suites); final recovery delta passed 50 default-feature tests / six suites, including all 13 assessment tests. Strict default workspace Clippy and fmt pass. No native sponsor run is claimed. See [setup](VENUE_ASSESSMENTS.md).
- [#70 kernel audit](https://github.com/HenryWashuHe/genesis/pull/70): **merged** as `64d6492`, reviewed source `e552a39`, exact composed tree `343ae1b`. All three original `c92f9d5` regressions pass unchanged: complete frozen gap fields, acquired pin/publication stability, consistent snapshot/event reads under a synchronized writer. Positive first-publication and installation controls remain accepted. Latest composition passed six history/regression tests and 37 scenario units; 65 core and 14 CLI checks passed on the prior composition with identical corresponding source. Strict core/server all-target Clippy/fmt pass. The earlier hold at `c1162d1` is resolved.
- [#71 gap intake UI](https://github.com/HenryWashuHe/genesis/pull/71): **merged** as `7baf9d3`, reviewed source `3d37503`. Both original regressions from `37dae9f` pass unchanged, along with all 62 web tests, typecheck and production build. Older polls cannot restore stale Attention; disabled intake keeps the acquisition identity and accurately describes configuration. Exact composed tree `8b0edde`; no backend changes or live browser claim.
- [#73 bounded refused requests](https://github.com/HenryWashuHe/genesis/pull/73): **merged** as `870cbd7`, reviewed source `22c2a22`, exact tree `2feec0b`. Three refusals per requirement across all capability names bound the loop. Composed review: 145 checks including two extra controls, strict scoped Clippy/fmt. Extra isolation, failed-proposal and planner restart controls preserved as `090d6fe` on `codex/review-gap-retry73-renamed`.
- [#74 planner busy-provider retries](https://github.com/HenryWashuHe/genesis/pull/74): **merged** as `8187430`, reviewed source `323a2dc`, exact tree `852e6fa` after the UI merge. 54 planner/evaluation tests plus four actual loopback HTTP checks, strict scoped Clippy/fmt. Elastic retries revalidate configuration; malformed success and definite authorization errors do not retry. Extra HTTP controls preserved as `71f816a` on `codex/review-planner-retry74-http`.

Claude/Eric owns the explicitly assigned opt-in gap `Acquired → Installed`
finalizer and kernel/planner/readiness/UI. Codex owns assessment/scenario
integration, continuing monitor subscriptions and the monitoring setup helper.
No review hold remains from the
#70/#71/#73/#74/#75 batch. Current GitHub heads supersede this snapshot. The earlier queue closeout
below is historical.

## Previous merged baseline

Reviewed main checkpoint: `b2aebcd173972755297d68a1e161e2113cea5181`.
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
| [#56 gap policy](https://github.com/HenryWashuHe/genesis/pull/56) | `00ad205` | `02cfc90`, exact tree; runtime identical to `cf591a3`, ten policy tests/strict server Clippy. Offline materializer at that source; durable driver is now merged in #66. |
| [#47 live gates](https://github.com/HenryWashuHe/genesis/pull/47) | `1857b38` | `c91424a`, exact computed merge tree; 14 offline tests including unchanged retry, terminal-result, independent checkpoint, shared quota and resume regressions. No provider calls. |
| [#51 demo runbook](https://github.com/HenryWashuHe/genesis/pull/51) | `dd33842` | `ae81688`, exact computed tree; original setup/acknowledgment/wording findings fixed, four relative links and whitespace checked. Historical #60 fallback was explicit; holdout support has since landed. Documentation only. |
| [#61 gap event log](https://github.com/HenryWashuHe/genesis/pull/61) | `e0c0d5d` | Reviewed #64 correction `245c3e1` / parent `c34ccc1`; 45 web tests, typecheck/build and component render/filter checks. Main web source equals reviewed source; backend/schema unchanged. |
| [#52 planner eval](https://github.com/HenryWashuHe/genesis/pull/52) | `aec4792` | `b887280` resolves the three unchanged negative probes. Integration `278ecfc`, exact tree: 12 tests and strict default workspace Clippy pass. Separate recorded replay: nine scenarios pass, zero skips/errors. No live model run. |
| [#63 planner lifecycle](https://github.com/HenryWashuHe/genesis/pull/63), [#57 gap wakeup](https://github.com/HenryWashuHe/genesis/pull/57) | `b91ad0a`, `f0d295a` | Exact final tree equals `7731aa3`. Tested `bffd82c` differs only by one additional queued-wakeup test file: 74 focused tests (41 planner, 16 core, five server, 12 eval), formatting and strict workspace Clippy pass. No planning on closed missions at entry/replay; in-flight cancellation not claimed. |
| [#60 portal holdout](https://github.com/HenryWashuHe/genesis/pull/60) | `ec5ec22` | `bfd5f88` fixes emitted example-index links and hashes. Separate example/verification generation preserves the ordinary reproducibility path. |
| [#26 gateway readiness](https://github.com/HenryWashuHe/genesis/pull/26), [#62 durable recovery](https://github.com/HenryWashuHe/genesis/pull/62) | `aadb15d` | #62 `e27ecf7` merged into #26 at `9935bd8`; fresh-schema retry/reopen and legacy migration regressions resolved. Main integration passed 304 server tests. No native readiness run claimed. |
| [#27 operator attach](https://github.com/HenryWashuHe/genesis/pull/27) | `574c5b8` | `29c9853` composed with current main and corrected #26: 69 focused tests with `serde_json/preserve_order`, strict default Clippy. Attachment retains the separate readiness gate. |
| [#28 Attention UI](https://github.com/HenryWashuHe/genesis/pull/28) | `0b91ddf` | `606b92b`; final composition `0862df8`, exact main tree. Clean install, 56 web tests/typecheck/build; evidence refresh and gap-event behavior retained. Attention checks are pure/SSR; no live-provider or browser-interaction claim. |
| [#66 durable gap driver](https://github.com/HenryWashuHe/genesis/pull/66) | `e2563ef` | Source `dd2d6b6`, exact main tree. Earlier `a66a298`: 310 server tests/30 suites, including 16 driver tests. Final runtime composition `2561be`: 152 default-feature tests/nine targets, strict workspace Clippy/fmt; final delta docs only. Actual loopback `serve_org` progresses a committed gap without manual steps. |
| [#29 CREAO readiness probe](https://github.com/HenryWashuHe/genesis/pull/29) | `ba4c920` | `5f5b776`, exact main tree. Final composition: 40 default-feature tests (16 gap, eight probe, 14 CLI, two configuration), strict workspace Clippy/fmt. Earlier scoped run: 122 tests with `serde_json/preserve_order`. Native status alone is not readiness; no live account run. |
| [#68 preserved Elastic scenario](https://github.com/HenryWashuHe/genesis/pull/68) | `2adaff4` | Extracted unchanged scenario from `9d63cc0`, source `182e22f`. One targeted repaired A→B scenario passed in 97.72 seconds, including Elastic search, organization-isolated evidence and restart. Final composition Clippy passes. Test only, loopback providers. |
| [#67 invariant tests](https://github.com/HenryWashuHe/genesis/pull/67) | `b2aebcd` | `eeddda5`, test only; 63 core checks, seeded 8,000-step sequences and six sampled negative controls, strict core Clippy/fmt. These controls do not cover every invariant. |

Strict Clippy checks above use default features. An additional planner integration Clippy run with the JSON-order
feature reported size lints and is not claimed green. Counts belong to their
listed sources, not an aggregate current-main test run. Review caches are separate
per checkout; the specialist/planner cache is exclusively
`/tmp/genesis-pr20-review-target`, while other checkouts use their own `target`.

The earlier capability stack #8–#10/#13–#19/#24 and CREAO execution receipts #32
are on main. Historical detailed evidence remains in
[the prior queue](https://github.com/HenryWashuHe/genesis/blob/a857a9f/docs/PR_REVIEW_QUEUE.md).

## Queue closeout and remaining product work

The reviewed queue is handled at this checkpoint. [#25](https://github.com/HenryWashuHe/genesis/pull/25)
was closed without merging or deleting its branch after all 17 combined-only merge
commits and remaining unique content were audited. No unique pending runtime
remained; its useful recording-proxy scenario
was preserved in #68. Its `2829025`/132-test and `1004934`/699-test results remain
historical. See [the combined checkpoint](COMBINED_INTEGRATION.md).

The merged gap driver stops at verified `Acquired`; publication and installation
remain separate. The next Codex core implementation is persistent CREAO mission
assessment backed by verified capability output and execution receipts. The mission
still needs the full planner-to-acquisition-to-publication and independent adoption
demonstration, persistent CREAO core work, and native sponsor receipts. Fixture execution, API presence, callback reachability
and a green PR do not establish those live outcomes. Coordinate ownership and
merge sequencing in [issue #4](https://github.com/HenryWashuHe/genesis/issues/4).
