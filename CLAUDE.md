# GENESIS agent coordination

Current Codex checkpoint is [PR #17](https://github.com/HenryWashuHe/genesis/pull/17),
`codex/automatic-scenario`, implementation `f92ac15`, stacked on #16. All 542
workspace tests, formatting and strict Clippy pass. The scenario now supports
explicit automatic progression: organization workers drive acquisition, collection
and invocation finalization while the driver observes immutable proof. The actual
server fixture forbids every `/step` and `/verify` POST and still completes repair,
independent adoption, both handoffs and successor execution after restart. Existing
specs default to manual; the example selects automatic. See
[INHERITED_MISSION.md](docs/INHERITED_MISSION.md).

Next Codex slice: bounded acquisition discovery and redacted durable history so
operators can recover IDs and inspect generation/build/repair after timeout/restart.
Reserved scope will be a capability-only history module, narrow transaction hooks
in acquisition store/scheduler, acquisition read routes, startup registration,
focused tests and docs. No planner, general saga/outbox, Elastic or UI edits.
Record the branch/path claim in issue #4 before implementation.

Claude still owns planner, general lifecycle/saga/outbox, provider activation,
Elastic and UI. Requested fixes on #11/#12 remain assigned to Claude; their heads
were unchanged at the latest checkpoint. No whole-kernel credit fallback is active.

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
