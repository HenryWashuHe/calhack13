# GENESIS agent coordination

> **Working repository: [genesis-history-experiment](https://github.com/HenryWashuHe/genesis-history-experiment).**
> Push and open PRs here; coordinate in [issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1).
> The owner moved active work here on October 23, 2026 (Pacific). Histories have
> different commit identities; preserve this repository’s ancestry and verify
> mapped-base tree deltas. See the [migration map](docs/REPOSITORY_MIGRATION.md)
> for legacy PR numbers and preserved work. The proposed sync script is held;
> do not assume it is ready. Historical checkpoints below retain old links.

## Current direction and coordination

**Networked recursive capability improvement with fixed model weights** is the
user-selected product framing. It supersedes the previous decision to relegate
recursive improvement to “What's next.” Distinguish implemented features from
unmerged work and vision using [implementation status](docs/IMPLEMENTATION_STATUS.md).
Accumulation, reuse/composition and improved capability development are different
claims; the last needs independent comparative evidence.

Event organization is discarded as the product/demo. Preserve existing venue
code/tests as historical integration fixtures; do not expand that product story
or use its completion as the new gate. The [invention-lab A → B → A demonstration](docs/INVENTION_LAB_DEMO.md)
is proposed, not implemented. Its simulator, executable development-method runtime
and recursive-benefit experiment remain open work.

**Ongoing work is resumed under [the repository goal](docs/ONGOING_GOAL.md).**
The existing hourly continuation is active. At this checkpoint the older app Goal
card remains paused because the goal tool cannot replace an unfinished record;
never falsely complete it to work around that limitation. The user's renewed
instruction authorizes scoped work under the revised goal, not the discarded
venue objective. Preserve local deployed-repair WIP until separately scoped.

Read the [goal's working cycle](docs/ONGOING_GOAL.md#working-cycle),
[current ownership map](docs/AGENT_HANDOFF.md#current-ownership-and-next-work)
and [issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1), including newer
branch/head updates, before editing or assigning work. Check goal alignment at
every milestone. All lower checkpoints are historical.

- Henry's core Claude: [#122](https://github.com/HenryWashuHe/genesis/issues/122),
  #116 corrections/new outcome classifier and #121 terminal-run retry; then
  development-method adoption contract. #116 is now `78a18c0`, independently
  unreviewed; do not repeat old-head findings as verified on its correction.
- Eric (`EricZhu-31`): [#123](https://github.com/HenryWashuHe/genesis/issues/123),
  #120 constitution audit; then invention-lab evidence UI design and isolated subtree.
- ACP Claude session: [#125](https://github.com/HenryWashuHe/genesis/issues/125),
  merged #114's outstanding regressions, then product-neutral command design.
- Codex: [#124](https://github.com/HenryWashuHe/genesis/issues/124), shared method,
  identity, receipt and comparison contracts. Preserve paused repair WIP separately.
- Evaluator/simulator: Henry’s core Claude is assigned the requested legacy
  [#126](https://github.com/HenryWashuHe/genesis/issues/126) lane: current proposal
  [#13](https://github.com/HenryWashuHe/genesis-history-experiment/pull/13), then
  only `experiments/invention-lab/evaluator/**` after interface agreement.
  Ines stays unassigned by user instruction; do not infer her identity.

The task/PR numbers in the preceding legacy lane descriptions refer to the old
repository unless explicitly linked here. Use the migration map for current PRs.

New method/adoption/UI implementation depends on agreed method and evaluator
contracts. Claim shared wiring explicitly; do not take another session's files.
Keep commits/PRs focused and pushed so other machines can integrate them.
Author-reported checks, independent local tests, native local processes and live
cloud execution are distinct evidence. Public Agentverse/ASI use and live sponsor
proof remain open. Keep the repository private unless the owner changes that.

## Historical checkpoint after #108

Main `74b99c3fabf6d17188b5bf53fba9cd83d828305d`, tree
`f75934c42770f902e40700079b50d9972806d220`. Read the current
[implementation handoff](docs/AGENT_HANDOFF.md) and newer scoped comments in
[issue #4](https://github.com/HenryWashuHe/genesis/issues/4) before taking work.
This supersedes the historical statuses below.

#84 and #103–#108 are merged, including native Fetch/coordinator composition,
ACP recovery commands and stopped-intent cleanup. #86 `c2dc523` and #91 `6776130`
remain held on preserved regressions. Claude/Eric retains kernel/planner/
readiness/adoption/UI; Codex continues opt-in automatic recovery intake in its
monitor/Fetch scope. The ongoing two-organization mission goal remains active.

The official Fetch track needs the complete workflow in ASI:One and a public
repository in addition to ACP/Agentverse. Current ACP supports fixed recovery
cases only. Do not claim public registration, ASI routing or live provider proof;
do not change private repository visibility without owner authorization. See
[the sponsor guide](docs/FETCH_SPONSOR_NOTES.md) for exact setup and limitations.

## Historical checkpoint after #101

Source basis: main `0a0c175aa452d2dfcdee617e6f1647a523f5cceb`, tree
`ec382ccbfabc37487d23e3962e955f49f295493c`, after #101. This checkpoint
supersedes the historical status and ownership notes below; exact newer GitHub
heads and the scoped handoffs in [issue #4](https://github.com/HenryWashuHe/genesis/issues/4)
take precedence. The ongoing two-organization mission goal remains active.

**Merged:** #78 finalizer, #80 finalizer UI, #88 settled-check display,
#89 planner policy names/sources/scopes, #97 console guard tests, and
#90/#92/#93/#94/#95/#96 saga, audit and planner changes. The six-PR Rust
composition at `97d75eb` (tree `f305ca60`) passed **608 tests across 68 targets**,
scoped strict Clippy and formatting. These are source-specific results, not a
new aggregate count for every later main commit.

- **#98 Fetch float identity:** merged `9f9a87d`, source `11703e2`.
  Canonical JSON float identity survives journal/receipt reconstruction;
  stale changed intents cannot block release or unrelated healthy work.
  **133 Fetch/verification tests**, including ten float cases, pass. Three
  baseline defects and two release regressions were reproduced before fixes.
- **#99 monitor intake:** merged `72cd014`, source `05d55e2`, tree `5e5cf477`.
  Persisted scans choose the newest eligible source within their captured
  backlog, including across pages/restart. Manual consumption and temporary
  CREAO absence no longer permanently halt valid intake; exact child recovery,
  stop/expiry and authority fences remain. **34 assessment + 117 server-library
  tests**, scoped strict Clippy and formatting pass. Baseline evidence was four
  failing regressions plus four passing controls. See [monitor semantics](docs/VENUE_MONITORS.md).

- **#82 adopt first:** merged `9a9c7a0`, source correction `ae00fa9`, tested
  composition `4c8c461`, tree `05767561`. The original same-owner
  delegation regression is unchanged and now passes. **41 focused tests**,
  scoped strict Clippy and formatting pass. The author branch
  `henry-claude/gap-adoption` was fast-forwarded to `4c8c461` to include current
  main; no history was rewritten. Claude should pull before continuing it.
- **#83 / #85 / #87:** merged as `b58ac99` / `090700b` / `8027428`.
  The two-organization integration passes; adoption UI passes **85 web tests**,
  typecheck and build. The local rehearsal reached synthesis, publication and
  installation in A, then discovery, checks and installation in B with no B
  synthesis. These are fixture-provider results, not a live planner/sponsor run.
- **#101 Fetch venue recovery:** merged `0a0c175`, source `093010b`, exact tree
  above. **255 focused Rust tests** (42 assessment, 135 server-library, 78 Fetch),
  the newly merged two-organization test, **34 Python tests**, scoped strict
  Clippy and formatting pass; independent source review is clean. Recovery
  preserves verified source/child identity and retires its local contract with
  bounded exact release delivery. Bridge queue acknowledgment does not prove
  peer release. See [the API and evidence boundary](docs/VENUE_RECOVERY.md).

**Open review queue:** #86 `83998b0` remains **held on a reproduced failure**.
A local check marked `Rejected` after suite drift can retain a native receipt
whose provider run is still `RUNNING`. Retry submits a second POST while an
independent GET confirms the first run remains active. Test-only `a497651` on
`codex/review-adoption86-rejected`, composed with then-current main at `c33c93f`,
reproduces this; it is loopback provider evidence, not a live sponsor run.
#91 `6776130` remains held on replay starvation under frequent nonstructural
events and missing revisions displayed as audited success. Draft #84 `3d3219c`
now passes **four preflight tests** and scoped strict Clippy; its original policy
validation defect is cleared. Its remaining hold is the runbook's presentation
of unreleased #86 retry behavior and the claim that a check is never submitted
twice (`docs/DEMO_RUNBOOK.md:311`), which the #86 regression contradicts.
See [the exact queue](docs/PR_REVIEW_QUEUE.md).

**Ownership:** Claude/Eric retains kernel, planner, readiness, adoption and UI,
including #86/#91 fixes. Claude's `henry-claude/requestable-adoption` claim covers
`crates/genesis-server/src/routes/capability_gaps.rs` requestable policy visibility,
`crates/genesis-planner/src/prompt.rs`, matching `RequestPolicy` / `Requestable`
deserialization and adjacent tests. The follow-up exposes adoption-only names
without acquisition sources/scope and uses the observation's public page URLs;
it does not change the coordinator, kernel or acquisition. See the
[scoped claim](https://github.com/HenryWashuHe/genesis/issues/4#issuecomment-6372394586).
Codex's monitor, Fetch float and #101 recovery implementations are merged.
Codex's next focus is composing the native Python bridge with the Rust recovery
coordinator in one fixture-backed integration run. Existing Python signed-process
and coordinator authenticated-boundary tests pass separately; the combined path
is not yet established. Coordinate before overlapping files. Preserve review
evidence and push tested increments; the active root reviewer sequences merges.

**Evidence and credentials:** no native sponsor run is established. Loopback
fixtures and local signed-process checks do not prove cloud execution or a
completed autonomous mission. The user has not supplied the required credentials:
`APIFY_TOKEN`, `CREAO_ACCOUNT_API_KEY`, `ELASTICSEARCH_URL`,
`ELASTIC_DATA_API_KEY`, `ELASTIC_KIBANA_URL`, `ELASTIC_MANAGEMENT_API_KEY`, and
`AGENTVERSE_API_KEY`. Setup also needs each provider's configured runtime and,
for synthesis/planning, the relevant model credentials. Follow
[LIVE_GATES.md](docs/LIVE_GATES.md); preserve actual agent/run evidence and a
genuine CREAO Share link separately. Never put credentials in handoff files.

## Historical checkpoints

Everything below preserves earlier source-specific results and assignments.
Statements such as “current,” “next” or “hold” below describe their original
checkpoint, not the authoritative status above.

## Historical implementation and ownership — October 22, 2026

Immutable Fetch receipts are implemented at `afbe0bf` (reviewed original
`6424bfb`). Exact frozen recruitment/consent/task messages are checked against
prepared outbox and authenticated Applied inbox records in one SQLite snapshot.
Receipt hashes survive later work/release/restart; construction is private and
Debug hides payloads. **64 Fetch crate tests**, including 13 receipt tests, strict
crate all-target Clippy and workspace fmt pass. Independent review is clean.
See [the API and trust boundary](docs/FETCH_RUNTIME.md#immutable-receipt-reconstruction).
This adds no server integration, native signature verification or domain verdict.

[PR #79](https://github.com/HenryWashuHe/genesis/pull/79) is merged as `1fc1b8e`,
reviewed source `135c839`, exact tree `c09245b`. Runtime `8741f38` registers one
durable subscription and observes both accepted assessment cycles. All 40 scenario
unit/HTTP checks and five full scenarios pass, with strict default workspace
all-target Clippy/fmt and clean independent review. Loopback evidence only.

Claude's [PR #78](https://github.com/HenryWashuHe/genesis/pull/78) is on review
hold at `7cb747c`. Composition `6be7dc2` on `codex/review-gap-finalizer78` preserves
five reproduced regressions and 11 passing controls: live requirement fencing,
weaker-suite acceptance, acceptance followed by delegation, and conflicting
same-pin publication. The ten original tests still pass. The two requirement
mutation tests use an explicitly documented Store commit seam; the acceptance,
delegation and publication paths use genuine public endpoints and loopback clients.
No production fix is included in that review branch.

Stacked [PR #80](https://github.com/HenryWashuHe/genesis/pull/80), `931192c`, also
awaits correction: global finalizer enablement with no per-gap record must not
claim work is queued. Its 66 original web tests/typecheck/build pass; `d369213`
adds the failing no-record regression and passing organization-switch/terminal
AttentionPanel control. Claude retains both implementations.

After those fixes, Claude is assigned the opt-in adopt-before-acquire gap
coordinator and narrow kernel transition in a separate PR. It must create one
durable adoption intent, retain uncertain outcomes, and permit synthesis fallback
only after a definitive persisted empty result. A correlation prefix cannot by
itself authorize a different selected capability ID. The detailed acceptance
criteria and ownership are in issue #4 comment `6354798168`; implementation has
not yet been reported.

Codex owns the immutable Fetch receipt slice, restricted to `genesis-fetch` and
its focused docs/tests. The implementation binds exact prepared requests to
authenticated Applied inbox evidence and preserves historical identity across
later tasks and release. No server bridge, kernel, planner,
readiness or UI changes are part of that slice. Native sponsor evidence remains
open. See the scoped handoff in issue #4 and the Fetch mission bridge plan.

[PR #77](https://github.com/HenryWashuHe/genesis/pull/77) adds opt-in durable venue
monitor subscriptions; **merged** as `1bb3c9f`, reviewed source `c889491`,
exact tree `3ad4418`. Runtime checkpoint `e699659` was based on main `3ea5ec4`.
One operator registration freezes the existing role/assignment/CREAO binding,
accepted capability and event policy. The server follows verified invocation
events, persists each exact child intent before creation, and uses the existing
assessment verifier for mission effects. Work is bounded by expiry and 1–16
intents. Unknown work retains its child identity; an accepted Reopen completes
the subscription with its replacement requirement, without completing the mission.
See [the API and limits](docs/VENUE_MONITORS.md).

Validation: **154 source-scoped Rust checks** across four targets: 113 server
library, 14 CLI, 22 assessment integration (including nine subscription tests),
and five full scenarios. The original 21 assessment checks passed together; the
final test-only addition passed with all nine subscription checks. The same
runtime passed strict default workspace all-target Clippy and formatting.
The crash regression recovers a committed child after its link write fails,
including a stopped subscription, without changing the job/deadline/native run.
A 205-event regression preserves the cursor through restart, skips a different
event input and processes a job verified after registration. Providers are
loopback; the subscription tests use trusted synthetic provisioning/publication.
Native sponsor evidence remains open.

[PR #76](https://github.com/HenryWashuHe/genesis/pull/76) adds the offline monitoring
setup helper; merged as `3ea5ec4`, reviewed source `1ff0c94`, exact tree
`fe419e7`. Implementation checkpoint `bfcfe00` was based on main `64d6492`.
It validates three immutable portal namespaces and atomically emits B's frozen
baseline/update suite and the complete strict scenario. The explicit policy must
accept the baseline and reject only the update's capacity. Public aliases and
non-string identities are refused; template access flags are preserved.
All **35 portal tests pass without skips**, including the actual Rust verifier
and scenario CLI. Independent review is clean; the exact documented walkthrough
also passed with temporary paths and a fake HTTPS origin. See the
[executable setup](docs/VENUE_PORTAL.md#prepare-continuing-creao-monitoring).
These checks make no native provider or unforeseen-update claim.

[PR #75](https://github.com/HenryWashuHe/genesis/pull/75) connects persistent CREAO
assessment to the two-organization scenario. Runtime checkpoint `f5136fe`, based
on reviewed main `8187430`, adds optional `venue_monitoring`: after independent
capability adoption and verifier retirement, the same newly created CREAO monitor
assesses a feasible baseline and a distinct capacity-cut snapshot. Its second
verified result creates one cited replacement requirement. Both proofs and the
same agent survive restart without extra native requests. See
[scenario setup and evidence limits](docs/INHERITED_MISSION.md).

Validation: **145 default-feature Rust checks** across four targets: 113 server
library tests (including 37 scenario unit/HTTP checks), 14 CLI tests, 13 assessment
integration tests and all five full scenario tests. Strict default workspace
all-target Clippy and formatting pass. Independent regressions failed before and
passed after fixes for bounded local-verification recovery, the final permitted
poll, and the kernel's valid owner-only handoff with no separate executor.

All providers in these tests are loopback. The full scenario uses the real CREAO
creation/client/runtime/journal, with an explicitly labeled creation-journal
readiness fixture; it does not prove a live gateway callback or cloud execution.
Both B source cases are configured before adoption. Actual sponsor runs, a genuine
CREAO Share link and the complete autonomous mission remain open. The underlying
assessment API from merged #72 remains unchanged; see
[its runtime contract](docs/VENUE_ASSESSMENTS.md).

Reviewed #70, #71, #73, #74 and #75 are merged. Main checkpoint `64d6492`
includes the persistent scenario and corrected audit. All three original audit
regressions from `c92f9d5` now pass unchanged: frozen gap binding, acquired pin /
publication stability, and consistent concurrent history reads. Six latest
history/regression tests and 37 scenario units pass; the prior composition's
identical core/CLI files passed 65 core and 14 CLI checks. Strict core/server
all-target Clippy and formatting pass. See [the review queue](docs/PR_REVIEW_QUEUE.md).

Claude/Eric owns kernel/planner/readiness/UI and the explicitly assigned opt-in
gap `Acquired → Installed` finalizer. Codex owns the monitor subscription and
its scenario integration, along with the completed operator setup helper.
No audit review hold remains from the previous batch.
Coordinate through [issue #4](https://github.com/HenryWashuHe/genesis/issues/4).
The ongoing mission goal remains active; historical notes below do not supersede
this ownership or current GitHub heads.

## Previous merge and ownership checkpoint — October 22, 2026

Reviewed main checkpoint: `b2aebcd173972755297d68a1e161e2113cea5181`.
The capability stack, specialist execution, planner and Elastic integrations,
evidence and gap UI, portal holdout generation, gateway readiness verification,
operator attach, Attention UI, the durable gap driver and CREAO readiness probe
have landed. The reviewed queue is closed out. See
[the review queue](docs/PR_REVIEW_QUEUE.md) for exact sources and checks.

Readiness #62 now fixes both fresh-schema recovery and legacy database migration;
it merged into #26 at `9935bd8`. Main's #26 integration passed 304 server tests.
The #27 composition passed 69 focused tests with `serde_json/preserve_order`
and strict default-feature Clippy. Main's #28 tree equals `0862df8`: clean
install, 56 web tests, typecheck and build pass, preserving the evidence-refresh
and gap-event tests. These are source-scoped checks, not a new full-workspace
total, native provider run or browser walkthrough.

The original #60 example-index mismatch is fixed and merged at `ec5ec22`.
`--holdout` supplies separate synthesis-example and verification pages; ordinary
portal generation still supports explicitly labeled reproducibility checks.
CREAO readiness probe #29 is merged at `ba4c920`, with 40 final default-feature
checks and strict workspace Clippy/fmt; its earlier scoped run passed 122 checks
with `serde_json/preserve_order`. The probe sends a durable challenge run, but
only a valid callback establishes readiness. Neither readiness nor operator
attachment establishes core mission work or a live sponsor demonstration.

Merged #66 implements the durable gap-to-acquisition driver. Initial source
`a66a298` passed 310 server tests across 30 suites, including 16 driver tests with
loopback provider responses and a real server-startup path. Final integration
`2561be` passed 152 default-feature tests across nine targets, strict workspace
Clippy and formatting; the later source delta was documentation only.
It freezes the request, model policy, suite and assignment, recovers one intent
across restart, and retains unknown-outcome and explicit read-retry protections.
`Acquired` records independent verification; publication, installation and
attachment remain separate. The next Codex core implementation is persistent
CREAO mission assessment backed by verified capability output and native execution
receipts. Readiness alone does not satisfy that work.

Combined draft #25 was closed without merging after all 17 combined-only merge
commits and remaining unique content were audited. No pending runtime was unique
to it. Its Elastic recording-proxy scenario
test was preserved in merged #68 (`2adaff4`): the targeted repaired A→B scenario
passed with real Rust Elastic retrieval/projection against loopback fixtures.
The old branch remains available; its 699-test total is historical. Merged #67
adds invariant tests: 63 core checks pass, including seeded sequences and six
sampled negative controls, with strict core Clippy/fmt. This is not an exhaustive
proof of every invariant.

Claude/Eric and Henry's Claude keep their scoped kernel/planner/readiness/UI work.
Codex handles capability/provider implementation, review and narrow integration;
no whole-kernel takeover is active. Coordinate in
[issue #4](https://github.com/HenryWashuHe/genesis/issues/4), push tested increments,
and leave parent/main merges to the active reviewer to avoid concurrent merge
races. Preserve source branches and exact-head evidence.

[CREAO sponsor evidence](docs/CREAO_SPONSOR_NOTES.md) still needs core mission work,
an actual agent ID and a genuine run share link. The
[Apify notes](docs/APIFY_SPONSOR_NOTES.md) retain meaningful-use requirements.
No live sponsor execution, public portal tunnel or independent-account Actor
access is established by these local tests. The ongoing mission goal remains active.

## Historical pre-merge checkpoint

The remaining sections preserve earlier checkpoints. References to “current”
below describe those earlier states, not the reviewed main checkpoint above.

Current Codex checkpoint is [PR #24](https://github.com/HenryWashuHe/genesis/pull/24),
`codex/automatic-adoption-scenario`, implementation `72157aa`, observer `cdfaf17`,
stacked on #19. Automatic mode now creates one B adoption intent with only B's
work and suite. B selects and verifies independently; the scenario compares the
selected artifact with A's publication afterward. Manual mode remains compatible.
See [INHERITED_MISSION.md](docs/INHERITED_MISSION.md).

All 594 workspace tests pass with `serde_json/preserve_order` (64 suites, zero
failed/ignored), formatting and strict default-feature all-target Clippy pass.
Guarded tests reject client-owned search/install/checks/acceptance and step/verify
writes, and prove evidence survives retirement/restart and successor execution.
These are loopback provider fixtures and explicitly labeled DevStub roles.

Claude has pushed updated #11/#12 and new #20–#23. The original planner and Elastic
regressions now pass, but follow-up planner recovery defects are reproduced in
`70c4edb` on `codex/review-planner-pr12-followup`; see the latest #12 review.
Specialist job authority review is in progress. Claude retains planner, general
lifecycle/saga/outbox, role activation, Elastic and UI. No whole-kernel credit
fallback is active. Codex continues with an isolated combined-stack integration
branch, limited to registrations/conflict resolution and integration evidence;
known-blocking source branches will not be merged to main by that work.

**Current next-work split:** see [NEXT_IMPLEMENTATION_SPLIT.md](docs/NEXT_IMPLEMENTATION_SPLIT.md).
At the user's request, Codex is taking capability execution/evidence integration
inside the kernel boundary. Claude keeps lifecycle/recovery, Elastic and UI work;
the document lists reserved paths so both machines can proceed without overlap.

Claude currently owns the kernel and UI. Codex's capability acceptance core and
Apify execution adapter were merged in [PR #1](https://github.com/HenryWashuHe/genesis/pull/1).
Source creation/builds, the checkpointed forge and Actor runtime are merged in
[PR #2](https://github.com/HenryWashuHe/genesis/pull/2). Codex has now pushed real
capability execution and independent local acceptance in
[PR #5](https://github.com/HenryWashuHe/genesis/pull/5), branch
`codex/capability-kernel-integration`. See [the runtime/API guide](docs/CAPABILITY_EXECUTION.md).

Claude's continuing assignment is [issue #4](https://github.com/HenryWashuHe/genesis/issues/4):
integrate asynchronous provider readiness, implement Elastic specialist
provisioning/retrieval and display the new capability evidence. The reproduced
PR #3 review findings and two merge-readiness follow-ups are resolved below.
Codex has pushed the CREAO runtime in [PR #6](https://github.com/HenryWashuHe/genesis/pull/6),
branch `codex/creao-runtime`; see
[the CREAO handoff](docs/CREAO_RUNTIME.md). It exposes durable pending/created/collected
progress, not a ready synchronous role. Coordinate the pending/readiness saga hook
after the PR #3 fixes. Codex owns Fetch in [PR #7](https://github.com/HenryWashuHe/genesis/pull/7) on `codex/fetch-recruitment`; see [FETCH_RUNTIME.md](docs/FETCH_RUNTIME.md).
The scoped capability-route ownership above supersedes the older general API
ownership record; the whole-kernel credit fallback remains inactive.

**The user authorizes Codex to take over kernel implementation if Claude runs out
of credits.** This is a fallback, not an ownership transfer already in effect.
UI ownership remains with Claude unless separately reassigned.

See [the handoff record](docs/AGENT_HANDOFF.md) for current ownership and the
information to leave at a checkpoint. Commit and push often: the agents work on
different machines, so unpushed changes are not visible to the other agent.

Latest integration: PR #5–#7 include the `efa3b4f` kernel fixes plus Codex's narrow
merge-readiness fixes `7a00da4` and `1460831`, announced in issue #4. Changed
delegations rotate assignment/epoch; per-change process-local gates serialize saga
drivers. Claude should merge updated main into active work, including the planner
branch, and avoid duplicating those fixes. This does not activate the kernel fallback.
Codex reserves `crates/genesis-fetch`, `services/fetch-bridge`, their tests, the
Fetch proof script and docs. The planner branch is Claude-owned; no planner edits
are included in this workstream.

Current Codex milestone: [PR #9](https://github.com/HenryWashuHe/genesis/pull/9),
`codex/capability-acquisition`, stacked on synthesis
[PR #8](https://github.com/HenryWashuHe/genesis/pull/8). Reserved paths are forge
acquisition/synthesis modules, CLI/tests/examples and focused docs. The durable
coordinator joins generation, build and independent verification with one semantic
repair; see [CAPABILITY_ACQUISITION.md](docs/CAPABILITY_ACQUISITION.md). Claude keeps
planner/kernel activation, Elastic and UI. The ongoing Codex goal continues across
milestones; each next path claim is recorded in issue #4 before shared edits.

Server integration is pushed in [PR #10](https://github.com/HenryWashuHe/genesis/pull/10),
`codex/server-acquisition`, code `aaefa12`, stacked on #9: new
capability acquisition store/runtime/routes/tests, narrow registrations and the
trusted publication-proof lookup. See [SERVER_ACQUISITION.md](docs/SERVER_ACQUISITION.md).
It binds acquisition to a committed requirement/assignment with durable count
quotas. Claude should call it only after the organization change commits. Planner,
core lifecycle/saga, provider-role activation, Elastic and UI remain Claude-owned.
