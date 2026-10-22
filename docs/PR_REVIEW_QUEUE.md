# PR review queue — October 22, 2026

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
was closed without merging or deleting its branch after auditing all 17 unique
commits. No unique pending runtime remained; its useful recording-proxy scenario
was preserved in #68. Its `2829025`/132-test and `1004934`/699-test results remain
historical. See [the combined checkpoint](COMBINED_INTEGRATION.md).

The merged gap driver stops at verified `Acquired`; publication and installation
remain separate. The next Codex core implementation is persistent CREAO mission
assessment backed by verified capability output and execution receipts. The mission
still needs the full planner-to-acquisition-to-publication and independent adoption
demonstration, persistent CREAO core work, and native sponsor receipts. Fixture execution, API presence, callback reachability
and a green PR do not establish those live outcomes. Coordinate ownership and
merge sequencing in [issue #4](https://github.com/HenryWashuHe/genesis/issues/4).
