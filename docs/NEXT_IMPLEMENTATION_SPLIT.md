# Next GENESIS implementation split

Assigned by Codex on October 21, 2026 at the user's explicit request to review
Claude's PR, redivide work, communicate the assignments and continue implementation.
This is a scoped integration handoff, not activation of the whole-kernel credit
fallback. The original PR #3 review targeted `21103d1`; the current resolution
checkpoint is recorded in [AGENT_HANDOFF.md](AGENT_HANDOFF.md).

Current Codex addition: durable capability acquisition in [PR #9](https://github.com/HenryWashuHe/genesis/pull/9),
`codex/capability-acquisition`,
stacked on source synthesis [PR #8](https://github.com/HenryWashuHe/genesis/pull/8).
Reserved paths remain forge modules/CLI/tests/examples and focused docs. The
coordinator joins generation, build and independent verification with one planned
semantic repair. See [CAPABILITY_ACQUISITION.md](CAPABILITY_ACQUISITION.md) for its
library interface and backend-compatible proof export. PR #3/#5/#6/#7 are merged;
Claude should merge main before integrating this boundary. Planner, role activation,
Elastic and UI remain Claude-owned. Codex continues successive capability/provider
milestones under the user's ongoing goal, checking this ownership split before each.

The next server slice is `codex/server-acquisition`, stacked on #9. Codex reserves
new `capability_acquisition_store.rs`, `capability_acquisition_runtime.rs`,
`routes/acquisitions.rs` and their tests, plus narrow startup/router configuration
and capability proof lookup changes. [SERVER_ACQUISITION.md](SERVER_ACQUISITION.md)
defines the HTTP contract, authority checks and persistent count quotas. No edits
to planner, lifecycle engine, saga, outbox, Elastic or UI are included.

## Codex: capability execution and verification integration

Branch: `codex/capability-kernel-integration`, based on PR #3 plus merged main.

Own the real pinned Apify executor, durable capability-run state, trusted evidence
mapping for publication/adoption, and their integration tests. Reserved paths:

- `crates/genesis-apify`, `genesis-verification`, `genesis-forge`;
- `crates/genesis-providers/src/lib.rs` and new capability/Apify modules;
- `crates/genesis-core/src/capability.rs` if the evidence boundary requires changes;
- `crates/genesis-server/src/routes/capabilities.rs`, capability-specific new
  modules, and the capability verification boundary in `registry.rs`;
- new capability integration tests; dependency/member additions required by them.

The first milestone connects exact artifact/source identity, typed run tickets,
restart-safe collection and independent acceptance to the real server path. Provider
status or an incoming passed flag must not establish semantic acceptance.

## Claude: kernel lifecycle/recovery fixes, organization intelligence and UI

Merge updated main into active branches, including `feat/planner`. Keep ownership
of `genesis-core` engine/change/jobs/model/lifecycle, server saga/store/outbox/general
job routes, `apps/web`, and their tests. Claude's `efa3b4f` fixes and Codex's narrow
merge-readiness commits `7a00da4`/`1460831` resolve the reproduced review defects
and delegation/concurrent-driver follow-ups. Historical review findings remain in
[the full review](reviews/PR-3.md).

Then integrate the pending/readiness saga hook described in the CREAO checkpoint
below, implement Elastic specialist provisioning and organizational evidence/capability
retrieval behind a new provider module, and show the actual verification/build/run
evidence in the UI. Codex owns the CREAO client/runtime and adapter; Codex owns the Fetch workstream described below. Each live sponsor path still needs its authenticated account smoke test.

Do not duplicate Codex's Apify execution/evidence path or edit the reserved capability
routes while that branch is active. Shared `genesis-server/src/lib.rs`, `app.rs`,
root Cargo files and generated schemas may need small integration registrations;
keep these changes narrow and record them with each pushed checkpoint.

## Coordination

Commit/push tested increments; unpushed work on the other machine is invisible.
Record exact paths and current commits here or in `AGENT_HANDOFF.md` before moving
work across this split. Repository comments communicate assignments, but do not
prove the other agent has read them. Dev stubs and local SDK tests remain visibly
separate from live sponsor evidence.


## Pushed integration checkpoint

[PR #5](https://github.com/HenryWashuHe/genesis/pull/5) contains the Codex milestone
through `0cfae06` plus follow-up checks. See [CAPABILITY_EXECUTION.md](CAPABILITY_EXECUTION.md)
for endpoint shapes, durable recovery and trust limits. [Issue #4](https://github.com/HenryWashuHe/genesis/issues/4)
is assigned to Eric for Claude's next work. Merge the integrated main into active
branches. Preserve the managed-capability guards in `routes/jobs.rs` and the
transactional general dispatch path. Shared changes are registrations/dependencies,
`Store::lock` visibility, CLI configuration and child-worker cancellation.


## Pushed CREAO checkpoint

The capability milestone is pushed through `130fd88` in PR #5. Codex has pushed
[PR #6](https://github.com/HenryWashuHe/genesis/pull/6) on `codex/creao-runtime`,
stacked on PR #5. Codex retains the new `crates/genesis-creao` client/runtime,
`genesis-providers/src/creao.rs`, related tests/docs and narrow registrations.
CREAO operations retain creation/run IDs, share a durable account limiter,
and recover known runs with GETs without treating native completion as authority.
Claude retains asynchronous kernel integration and Elastic/UI. Codex owns the Fetch workstream below.

The current generic provisioning saga cannot represent a pending CREAO creation
without failing it. PR #6 exposes typed progress and durable operations at the
provider boundary. Claude owns the pending/readiness saga hook next, after the
PR #3 lifecycle/recovery fixes; follow [CREAO_RUNTIME.md](CREAO_RUNTIME.md) for the
contract. Preserve created native IDs, independently prove readiness, and check
live assignment authority before every advance/result. No saga hook or live
provider activation is claimed by PR #6.

## Fetch discovery and independent recruitment

Codex owns [PR #7](https://github.com/HenryWashuHe/genesis/pull/7),
`codex/fetch-recruitment`, based on PR #6 after both provider branches
absorbed Claude’s `efa3b4f` review-fix checkpoint. Reserved paths are
`crates/genesis-fetch`, `services/fetch-bridge`, `scripts/demo-fetch-recruitment.py`,
Fetch tests/docs and required Cargo membership/lockfile changes.

The Rust runtime owns negotiation and task evidence; the Python process uses
native Fetch signatures and durable private transport. The local proof has two
separate organizations recruit one independently running specialist, restart
processes, release A, and continue B. See [FETCH_RUNTIME.md](FETCH_RUNTIME.md).
No automatic kernel role activation, authenticated public discovery or registration
is claimed. Claude retains kernel/saga/outbox integration, CREAO readiness, Elastic,
planner and UI. The A→B→A delegation-fencing follow-up is resolved by `7a00da4`;
`1460831` adds the per-change process-local saga guard. Neither activates live
CREAO or Fetch roles: those still require the asynchronous kernel hook.
