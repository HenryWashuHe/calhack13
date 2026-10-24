# GENESIS agent coordination

## Current merge and ownership checkpoint — October 22, 2026

Capability PRs #8/#9/#10/#13/#14/#15/#16/#17/#18/#19/#24 are merged to `main`
at `4ecc397`. Their resulting tree matches tested #24 exactly: 594 Rust tests,
formatting and strict Clippy. The user authorizes merging ready PRs; this does not
resolve findings on other heads. [Current queue](docs/PR_REVIEW_QUEUE.md).

Hold #11/#12/#20/#21/#22/#26/#27/#28 for their source fixes, #23 for stale handoff
content, and drafts #25/#29 for dependencies. Claude/Eric keeps those source fixes;
Codex keeps capability/provider implementation and narrow integration. No whole-kernel
credit fallback is active. The combined draft's 699-test evidence is separate from
main and does not resolve its thirteen reproduced negative review regressions.

[CREAO sponsor evidence](docs/CREAO_SPONSOR_NOTES.md) still needs core mission
execution, an actual agent ID and run share link. [Apify notes](docs/APIFY_SPONSOR_NOTES.md)
retain its meaningful-use and credit information. No live sponsor proof is claimed.

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
