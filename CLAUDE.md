# GENESIS agent coordination

## Current implementation and ownership — October 22, 2026

Persistent CREAO mission assessment is implemented in [PR #72](https://github.com/HenryWashuHe/genesis/pull/72),
runtime source `0dc0986be0248d1f7b5a96ded7a1c5cbe1795089`, based on main `72e9ef1`.
A continuing specialist receives frozen, backend-verified venue facts and event
constraints. Its native receipt and five domain checks gate an atomic observation
and, when needed, one actionable replacement requirement. The same agent handles
later updates across restart; its responsibility remains open. See
[the assessment API and evidence limits](docs/VENUE_ASSESSMENTS.md).

Source `1a340a9` passed **858 Rust tests in 90 nonempty suites** (109 targets
including empty suites), zero failed/ignored. The final missing-runtime recovery
fix then passed **50 default-feature tests in six affected suites**, including all
13 assessment integration tests, plus strict default workspace Clippy and fmt.
It preserves queued dispatch when a restart lacks CREAO instead of allowing a
stub acknowledgement to strand the job. These are loopback/provider-fixture
checks; native sponsor execution and genuine Share-link evidence remain open.

New reviews are held: **#70** at `c1162d1` needs complete frozen gap/pin checks,
a consistent history read transaction, and its Clippy fix; unchanged test-only
regressions are on `codex/review-kernel-audit70-refusal` at `c92f9d5`.
**#71** at `ba4e00ac` needs stale poll suppression and accurate disabled-intake
wording; preserve `37dae9f` on `codex/review-gap-intake71`. Exact findings and
source ownership remain in [the queue](docs/PR_REVIEW_QUEUE.md) and
[issue #4](https://github.com/HenryWashuHe/genesis/issues/4).

Claude/Eric keep kernel/planner/readiness/UI and those source fixes. Codex next
connects verified CREAO assessments to the two-organization scenario and its
report, without duplicating planner or UI work. The ongoing mission goal remains
active. Use current GitHub heads before integrating; the checkpoints below are
historical and do not supersede these ownership notes.

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
