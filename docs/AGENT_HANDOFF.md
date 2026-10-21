# GENESIS implementation handoff

Updated: October 21, 2026.

## Current ownership

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

## Kernel handoff record

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
- **Elastic (branch `feat/elastic`, stacked on `feat/kernel-ui`):** new crate `crates/genesis-elastic` (projection, retrieval, Agent Builder `RoleProvisioner`, test fakes behind feature `fake`), `genesis-server/src/projection.rs`, `routes/search.rs`, and small registrations in `app.rs`, `lib.rs`, `main.rs`, `routes/mod.rs`. See [ELASTIC.md](ELASTIC.md). Fake-tested only; live smoke test pending. Merging with PR #5 (`0cfae06`) gives trivial registration conflicts in `Cargo.toml`, `main.rs` and `routes/mod.rs`; resolved locally, 265 tests passed.
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
