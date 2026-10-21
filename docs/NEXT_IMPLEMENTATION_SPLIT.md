# Next GENESIS implementation split

Assigned by Codex on October 21, 2026 at the user's explicit request to review
Claude's PR, redivide work, communicate the assignments and continue implementation.
This is a scoped integration handoff, not activation of the whole-kernel credit
fallback. PR #3 head under review: `21103d1`.

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

Continue on `feat/kernel-ui`; merge main to receive the forge work. Keep ownership
of `genesis-core` engine/change/jobs/model/lifecycle, server saga/store/outbox/general
job routes, `apps/web`, and their tests. Address reproduced review defects in those
areas first. Review findings and reproduction instructions will be posted on PR #3.

Then implement Elastic specialist provisioning and organizational evidence/capability
retrieval behind a new provider module, and show the actual verification/build/run
evidence in the UI. Preserve the Fetch/CREAO ownership boundaries from the team
workstreams; their live integrations still need explicit owners and account smoke tests.

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
is assigned to Eric for Claude's fixes and next work. Merge main into `feat/kernel-ui`
to absorb already-merged PR #2; retarget the stacked integration PR to main once
PR #3 merges. Preserve the small managed-capability guards in `routes/jobs.rs` while
fixing general dispatch atomicity. Shared changes are registrations/dependencies,
`Store::lock` visibility, CLI configuration and child-worker cancellation.


## Next Codex goal: persistent CREAO specialists

The capability milestone is pushed through `130fd88` in PR #5. Codex continues on
`codex/creao-runtime`, reserving the new `crates/genesis-creao` client/runtime,
`genesis-providers/src/creao.rs`, related tests/docs and narrow registrations.
CREAO operations will retain creation/run IDs, share a durable account limiter,
and recover known runs with GETs without treating native completion as authority.
Claude retains all issue #4 fixes and Elastic/UI. Fetch remains unassigned.

The current generic provisioning saga cannot represent a pending CREAO creation
without failing it. Codex will expose typed progress and durable operations at the
provider boundary; the pending/readiness integration hook must be coordinated with
Claude rather than hiding an asynchronous create inside an unrecoverable call.
