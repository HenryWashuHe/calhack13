# GENESIS agent coordination

## Current merge and ownership checkpoint — October 22, 2026

Main is `f51d4d9`, including the capability stack, CREAO execution receipts #32,
[specialist execution #20](https://github.com/HenryWashuHe/genesis/pull/20) with
fixes #36/#41, and [venue portal #35](https://github.com/HenryWashuHe/genesis/pull/35)
with output-path fix #40. #20 is no longer a blocker for the readiness stack.
[Current queue](docs/PR_REVIEW_QUEUE.md).

Specialist integration `9bac3e1` passed 620 Rust tests across 69 suites before the
final provider-guard correction. Final source `5798cb4` passed 53 affected tests
across six suites, formatting, strict workspace all-target Clippy, 17 web tests
and the production build, with no schema/type drift. Main's #20 merge tree matches
that final source. The subsequent portal merge matches the expected combined tree;
16 portal tests pass with zero skipped, and both suites passed Rust verification
against independently fetched A/B pages. These checks do not constitute a new full-workspace total for
current main.

Hold #11/#21. The reviewed #39 head `49ca9ac` reproduces the Elastic capability-routing
failure; updated head `1bf7304` is under review. Updated #38 (`ae3879b`) passed 28 focused tests, formatting and strict
focused Clippy, and merged into #12 at `d1a5e1d`; no recorded #12 source blocker
remains, but its main integration at `0cad0c4` is still being tested. #42 (`c2b0f11`)
passed 16 focused tests and strict Clippy, then merged into #22 at `c6ccee4`; #22's
own findings are resolved, with #11/#12 dependencies and integration still pending.
Readiness fix #37 still has a durable mismatch-reconciliation
failure; #26 and descendants #27/#28/#29 remain pending. #27/#28's own reported
findings are resolved.
Claude/Eric and Henry's Claude retain their scoped source fixes. Codex owns
capability/provider implementation, review and narrow integration; no whole-kernel
takeover is active. The combined draft now includes main at `f5e747c` and passes
71 affected Rust tests across nine suites, formatting and strict workspace
all-target Clippy. Earlier full combined test counts remain historical and do not apply to current main.

[CREAO sponsor evidence](docs/CREAO_SPONSOR_NOTES.md) still needs core mission execution,
an actual agent ID and run share link. [Apify notes](docs/APIFY_SPONSOR_NOTES.md) retain its
meaningful-use and credit information. The portal has not been exposed through a
public tunnel, and no live provider or sponsor proof is claimed.

## Combined draft checkpoint

Sponsor submission evidence: [CREAO notes](docs/CREAO_SPONSOR_NOTES.md) and
[Apify notes](docs/APIFY_SPONSOR_NOTES.md). CREAO requires core agent work plus an
agent ID and genuine run share link. The current Apify monitor invocation and
CREAO readiness probe do not close that mission-execution gap.

Latest assembled source: [draft PR #25](https://github.com/HenryWashuHe/genesis/pull/25),
`codex/combined-integration` at `3961ecc`. It adds main's CREAO execution receipts
#32, #28's UI fix #31, and #27's sibling-agent fix #33. Receipt/UI checks at
`8014683` pass: 112 Rust tests/17 suites and 37 web tests plus production build.
The later sibling fix passes its 19 focused tests/four suites; formatting and
strict all-target Clippy pass. The previous full Rust run at `1004934` passed 699 tests;
it was not repeated for this narrow integration. No live sponsor evidence is claimed.
See [the integration matrix](docs/COMBINED_INTEGRATION.md) and [review queue](docs/PR_REVIEW_QUEUE.md).

This branch remains draft: twelve reproduced failing regressions across
#11/#12/#20/#26 remain on separate test-only branches, with source findings
on #21/#22. #27/#28's own findings are resolved but their backend dependencies
remain held. Henry's Claude is handling scoped #27/#28 fixes; Claude/Eric owns the other
source fixes. No whole-kernel takeover is active. The ongoing mission goal remains
active; CREAO core mission work is still distinct from readiness and provenance.

### CREAO provider readiness addition

[Draft #29](https://github.com/HenryWashuHe/genesis/pull/29), source `c1e6ba4`,
adds the challenge-bound durable CREAO run, submission-only runtime path,
purpose isolation, redacted Debug output, and `GENESIS_PUBLIC_URL` registration.
The focused branch passed 407 Rust tests, formatting and strict Clippy. See
[CREAO_READINESS.md](docs/CREAO_READINESS.md) for configuration and evidence limits.
The combined checks above include this addition. No live account or installed
skill was exercised. Existing source review findings and ownership remain
unchanged.

### Previous capability checkpoint

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
