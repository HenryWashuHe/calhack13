# GENESIS implementation handoff

Updated: October 21, 2026.

## Current integration checkpoint

Codex has pushed [PR #13](https://github.com/HenryWashuHe/genesis/pull/13),
`codex/inherited-mission`, code/tests `6ba8712`, setup documentation `0c339b2`,
stacked on #10. The Rust `genesis scenario` driver uses public HTTP APIs for a
requirement-backed specialist, acquisition with optional repair, exact publication,
B's independent discovery/acceptance, both responsibility handoffs and successor
execution. Capability-owned authority checks now enforce leases and mission
state/deadlines while preserving already committed acceptance after verifier
retirement. See [INHERITED_MISSION.md](INHERITED_MISSION.md).

All **471 Rust workspace tests** pass with `serde_json/preserve_order`, zero
failed/ignored; formatting and strict all-target Clippy pass. The new full-chain
fixture makes two model calls/Actors/builds for one semantic repair, six A
verification runs and six B runs including successor execution after reopening
B's SQLite DB. B has no acquisition/model client. Native provider responses are
canned loopback fixtures; created roles are labeled development stubs. Neither
ledger is changed, and collected invocations remain `VERIFYING`.

The overarching goal remains active. Next Codex core work is scoped semantic
finalization of fresh invocation outputs: a single-case verifier distinct from
full-suite acceptance, a backend-frozen installation/suite/acceptance binding and
an immutable local result proof. This must not reauthorize the retired verifier or
manufacture a booking. New exact path claims belong in issue #4 before edits.

Claude has now opened [PR #11](https://github.com/HenryWashuHe/genesis/pull/11)
(`feat/elastic`, `4d4949c`) and
[PR #12](https://github.com/HenryWashuHe/genesis/pull/12)
(`feat/planner`, `5f03bb5`), both based on main `5c4dd1a`. Codex is reviewing them
without taking over their implementation paths. The capability dependency stack
is #8 → #9 → #10 → #13. Planner/lifecycle/saga, provider activation, Elastic and UI
remain Claude-owned; the whole-kernel credit fallback remains inactive.

## Prior integration checkpoints

Codex has pushed [PR #10](https://github.com/HenryWashuHe/genesis/pull/10),
`codex/server-acquisition`, code checkpoint `aaefa12`, stacked on #9. New server
endpoints create/step/report a SQLite acquisition bound to a committed requirement,
role authority, assignment/epoch, lease and deadline. Provider intent and durable
count quotas commit together before I/O. Stale authority permanently fences work
while preserving provider receipts. Verified acquisitions can publish via
`verification_ref: acquisition:<id>` after fresh authority checks and independent
reverification. See [SERVER_ACQUISITION.md](SERVER_ACQUISITION.md).

All **453 Rust workspace tests** pass with `serde_json/preserve_order`, zero
failed/ignored. Formatting and strict all-target Clippy pass; the seven CLI tests
also pass after the final argument-struct cleanup. The 27 new tests cover store,
runtime, HTTP and CLI, including complete acquisition and actual HTTP publication
to a loopback registry, restart, quota persistence, concurrent steps, revoked
permissions, prompt/status privacy and foreign proofs. No live sponsor or paid
model calls were made. Retained acquisition records require the matching embedded
scaffold; count quotas are organization-local action bounds, not dollar limits.

The overarching goal remains active. Next Codex work should connect this acquired
proof to the two-organization inheritance demonstration and close any reproduced
capability-runtime authority gaps without editing Claude's planner/saga/UI paths.
Check current remote branches and issue #4 before claiming the next slice. Main
remains `5c4dd1a`; the current open dependency stack is #8 → #9 → #10. Claude's
`feat/planner` remains `caac0dc` at this checkpoint; that does not establish credit
exhaustion or authorize a whole-kernel takeover.

Codex has pushed [PR #9](https://github.com/HenryWashuHe/genesis/pull/9),
`codex/capability-acquisition`, code checkpoint `6853f16`, stacked on PR #8. One
`AcquisitionSession` now carries generation, Actor packaging/build and independent
verification, with one preplanned semantic repair. The CLI exports a reverified
backend-compatible ForgeSession proof. The normalized Apify receipt bound closes
JSON expansion beyond checkpoint headroom. See
[CAPABILITY_ACQUISITION.md](CAPABILITY_ACQUISITION.md) for commands and contracts.
All **426 Rust tests** pass with `serde_json/preserve_order`, zero failed/ignored;
workspace formatting and strict all-target Clippy pass. No live provider calls
were made. Use the same embedded scaffold/binary through each acquisition.

The user has requested ongoing autonomous implementation across milestones.
The next slice announced at that checkpoint was mission-bound server acquisition,
now implemented in PR #10 above: durable SQLite
intent/step/status, current assignment/lease/deadline checks, persistent count
quotas and trusted publication-proof lookup. New capability modules/routes and
narrow app registrations/evidence lookup are Codex-owned; Claude keeps planner,
core lifecycle/saga, asynchronous provider activation, Elastic and UI. Allocation
budgets are not a provider-spend ledger; the next slice must not claim dollar
billing enforcement. Ownership and exact integration checkpoints are also recorded
in [issue #4](https://github.com/HenryWashuHe/genesis/issues/4).

PR #3/#5/#6/#7 are merged on main at `5c4dd1a`. Codex has pushed source
synthesis in [PR #8](https://github.com/HenryWashuHe/genesis/pull/8), branch
`codex/capability-synthesis`, code checkpoint `f91d6ef`, in the forge modules/CLI/tests, with
public-example inputs, a durable one-shot model adapter and exact generated-source
handoff. [CAPABILITY_SYNTHESIS.md](CAPABILITY_SYNTHESIS.md) defines the interface
and remaining live gates. Claude retains asynchronous role activation, Elastic,
planner and UI; this does not activate the whole-kernel fallback.
The combined workspace passes 395 Rust tests with `serde_json/preserve_order`,
workspace formatting and strict all-target Clippy. The 29 new tests cover the
model adapter/state and CLI handoff. No paid model or cloud Actor calls were made.

Codex reviewed PR #3 at `21103d1`, requested changes with reproduced findings in
[the review](reviews/PR-3.md), and assigned [issue #4](https://github.com/HenryWashuHe/genesis/issues/4)
to Eric/Claude. The [current split](NEXT_IMPLEMENTATION_SPLIT.md) supersedes the
historical broad API ownership and next steps below.

Codex has pushed core capability integration in [PR #5](https://github.com/HenryWashuHe/genesis/pull/5),
branch `codex/capability-kernel-integration`, initial server checkpoint `0cfae06`.
It includes the real Apify provider, durable request/ticket recovery, backend forge
proofs, immutable local installation pins, independent acceptance runs and API
forgery/idempotency guards. See [CAPABILITY_EXECUTION.md](CAPABILITY_EXECUTION.md).
The branches include Claude's review-fix checkpoint `efa3b4f` and two narrow Codex
merge-readiness fixes, reserved in [issue #4](https://github.com/HenryWashuHe/genesis/issues/4#issuecomment-6326857019):

- `7a00da4`: changing the delegated executor creates a fresh assignment and epoch,
  preserving the accountable owner and lease. A→B→A cannot revive A's old grant;
  old owner work is also fenced and must be redispatched. Repeating the same
  executor keeps the current grant. Epoch exhaustion rejects before mutation.
- `1460831`: shared process-local per-change gates serialize run, recovery and
  retry drivers. Different changes can still provision concurrently. Cancellation
  releases the gate and recovery reuses a saved native binding. This is not a
  cross-process lock or a guarantee that an unknown native POST is safe to replay.

Claude retains CREAO/Fetch asynchronous kernel integration, Elastic, planner and UI.
Merge the updated main into active work; do not reimplement these two fixes.
No whole-kernel takeover is active. Historical review/checkpoint notes below retain
the limitations known when they were written; this section supersedes them.

Codex's CREAO implementation is in PR #6; [CREAO_RUNTIME.md](CREAO_RUNTIME.md)
records its durable provider boundary and pending readiness hook. Codex also owns
Fetch in [PR #7](https://github.com/HenryWashuHe/genesis/pull/7),
branch `codex/fetch-recruitment`: `genesis-fetch`, `services/fetch-bridge` and the
signed three-process/two-organization proof. [FETCH_RUNTIME.md](FETCH_RUNTIME.md)
documents exact commands, recovery, release isolation and the next kernel hook.
All live-account smoke gates remain open. Repository assignments communicate the
split but do not prove the other machine has read them.

## Ownership before the scoped integration handoff

| Area | Current owner | Status |
| --- | --- | --- |
| Organization kernel, shared runtime contracts, database and application API | Claude | Assigned; no takeover recorded |
| UI | Claude | Assigned; unchanged by the kernel fallback |
| `crates/genesis-verification`, `crates/genesis-apify` and their checks | Codex | Initial implementation merged in [PR #1](https://github.com/HenryWashuHe/genesis/pull/1) |
| `crates/genesis-forge`, `actors/venue-extractor-template`, capability scripts/docs | Codex | Source/build/runtime and durable coordinator work in [PR #2](https://github.com/HenryWashuHe/genesis/pull/2), branch `codex/capability-forge` |

The user has explicitly authorized **Codex to implement the kernel if Claude
exhausts its credits**. No further user approval is needed for that conditional
handoff. Codex should continue the existing implementation and its tests from
the latest pushed checkpoint. Remote inactivity alone does not establish that
Claude has stopped or exhausted credits.

## Leave a usable checkpoint

While credits remain, commit and push working increments. When handing off,
update the record below with the branch and commit, changed paths, completed
behavior, failing checks, pending work and any important implementation choices.
Keep credentials out of the record. If work cannot be pushed, explicitly say
which changes exist only on the other machine.

Once the credit-exhaustion handoff is known, Codex should fetch remote branches,
read this record and the latest kernel diff, then record and push the ownership
change before editing kernel files. Claude should consult that record before
resuming kernel work. This keeps the two machines from implementing the same
component concurrently; it does not add an approval step.

## Historical kernel checkpoint (superseded by the current split)

- **State:** Claude owns kernel and UI; fallback authorized, not activated. Recorded October 21, 2026.
- **Kernel branch / latest commit:** `feat/kernel-ui`, code checkpoint `72cb13e` (main and PR #2 merged in; this record is the commit after it). Open as [PR #3](https://github.com/HenryWashuHe/genesis/pull/3).
- **Files currently being changed:** root `Cargo.toml`/`Cargo.lock`, `crates/genesis-core`, `crates/genesis-providers`, `crates/genesis-server`, `schemas/`, `apps/web`, `scripts/dev-scenario.mjs`, `docs/DEV.md`. Codex-owned crates are untouched except that they are now workspace members.
- **Completed behavior:**
  - `genesis-core` (pure, no I/O): typed IDs, explicit lifecycle tables, `OrgState`, the TECH_SPEC §5 mutation vocabulary, validate/commit with invariants (unique accountability, constitution-bounded authority, retirement handoff, epoch fencing, exact capability versions), capability manifests with a publication bar, domain events.
  - `genesis-providers`: `RoleProvisioner` and `CapabilityExecutor` traits, a labeled `DevStub` (bindings `provider: dev_stub`, IDs `devstub-<provider>-…`), `FailingProvisioner`.
  - `genesis-server` (`genesis` binary): Axum routes from TECH_SPEC §10, SQLite snapshots per revision with events/outbox in one transaction, organization-change saga that persists each provider binding before advancing and resumes after restart, outbox worker, SSE with `Last-Event-ID`, operator/agent/adapter tokens, separate capability registry process, `genesis dev` (registry + Org A + Org B).
  - `apps/web`: console with mission, organization tree (pending roles hatched, retired roles kept, revision scrubber) and evidence log; types generated from `schemas/`.
- **PR #3 review findings ([docs/reviews/PR-3.md](reviews/PR-3.md)):** all 13 addressed, each with a regression test that asserts the intended behavior.
  - 1–3, 12, 13 (fencing, completion, overflow, requirement owner, bootstrap cycles): `crates/genesis-core/tests/review_regressions.rs`, debug and release.
  - 4–8 (atomic dispatch and submission, readiness identity and deadline, SSE catch-up): `crates/genesis-server/tests/review_durability.rs`.
  - 9–11 (snapshot ordering, reconnect reload, historical revision labels): `apps/web/src/lib/snapshot.test.ts`.
  - Schema change: `OrgChangeRecord.ready` and `provisioning_failed.provider_agent_id` (both optional on read).
  - **For Codex (reserved path):** `genesis_providers::ProvisionRequest::config_digest()` still hashes `Value::to_string()`; switch it to `genesis_core::digest::canonical_sha256`. Known limitation: a retried submission and restart recovery can drive the same change concurrently; add a per-change in-flight guard before live providers are wired.
- **Checks run at `72cb13e`:** `cargo test --workspace` 199 passed, also with `--features serde_json/preserve_order`; `genesis-core` release tests pass; clippy `-D warnings` clean; web `tsc`, vitest 16 passed and `vite build` succeed. `scripts/check-capability-core.sh` passes every Rust step; its final Actor step needs Node 24 (this machine has 22).
- **Earlier checks:** `cargo test --workspace` 122 passed (49 kernel + 73 capability), `cargo clippy --workspace --all-targets -- -D warnings` clean, `scripts/check-capability-core.sh` passes, `apps/web` vitest 7 passed and `vite build` succeeds. `node scripts/dev-scenario.mjs` completes the full A → B cycle against dev stubs. No live provider calls have been made.
- **Next implementation steps:**
  1. Implement `CapabilityExecutor` with `genesis-apify` (pinned build, persisted run tickets) and replace the registry's own `VerificationReport` checks with `genesis-verification` acceptance evidence when publishing and when Org B runs local acceptance.
  2. Replace keyword registry search with Elastic retrieval; add real `RoleProvisioner`s for Elastic, CREAO and the Fetch bridge as those workstreams land.
  3. UI: show verification evidence and Apify run tickets on capability rows.
- **Unpushed work or recovery notes:** None once this branch is pushed. Start with `docs/DEV.md`. Kernel entry points: `crates/genesis-core/src/engine.rs` (rules), `crates/genesis-server/src/saga.rs` (change lifecycle), `crates/genesis-server/src/routes/`.

## Available capability implementation

The initial verification and execution implementation is on main at merge
`dbb17f3`. [PR #2](https://github.com/HenryWashuHe/genesis/pull/2) adds Actor source
upload, source-checked builds, the TypeScript runtime and a checkpointed Rust
forge. The [forge guide](FORGE_INTEGRATION.md) describes the new source-to-result
path; the [verification guide](VERIFICATION_INTEGRATION.md) describes the oracle
and execution receipt boundary.

Run `bash scripts/check-capability-core.sh` for local validation. PR descriptions
record the latest check counts and limitations. Live Apify smoke testing is still
pending; hosted CI needs workflow-write permission. Codex is not editing kernel
models, SQLite/outbox state, Elastic integration or UI in this branch. The kernel
must persist sessions before effects, enforce budgets and leases, publish approved
packages, and decide whether to install them in either organization.

The [kernel integration checkpoint](KERNEL_CAPABILITY_HANDOFF.md) records a passing
temporary combined workspace (181 tests plus one documentation example), required
executor/report wiring, and two optional JSON feature-unification findings for
Claude. This is not a kernel ownership transfer.
