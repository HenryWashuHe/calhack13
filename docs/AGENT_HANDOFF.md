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

- **State:** Claude owns kernel, UI and the mission planner; fallback authorized, not activated. Updated October 21, 2026.
- **Kernel branch / latest commit:** `feat/kernel-ui` is in PR #3. Planner work is on `feat/planner`, branched from `feat/kernel-ui`; its latest commit is the one that updates this record.
- **Files currently being changed:** `crates/genesis-planner/` (new), `docs/PLANNER.md`, `.env.example`, and a small kernel addition: `EventKind::PlannerDecision` plus `POST /api/v1/planner/decisions`. Codex-owned crates are untouched.
- **Completed behavior:**
  - Kernel and UI as described in PR #3 (`docs/DEV.md`).
  - Mission planner: watches the event log, sends a domain-neutral prompt with mission, roles, responsibilities, constitution and the new evidence to any LLM, parses untrusted JSON into typed operations, records the requirement, submits the change with the agent token, feeds kernel rejections back (max 3 model calls), and logs a `planner_decision` event every time. Model provider is pending (Anthropic, OpenAI, Gemini or OpenAI-compatible via env vars).
- **PR #3 review findings ([docs/reviews/PR-3.md](reviews/PR-3.md)):** all 13 addressed, each with a regression test that asserts the intended behavior.
  - 1–3, 12, 13 (fencing, completion, overflow, requirement owner, bootstrap cycles): `crates/genesis-core/tests/review_regressions.rs`, debug and release.
  - 4–8 (atomic dispatch and submission, readiness identity and deadline, SSE catch-up): `crates/genesis-server/tests/review_durability.rs`.
  - 9–11 (snapshot ordering, reconnect reload, historical revision labels): `apps/web/src/lib/snapshot.test.ts`.
  - Schema change: `OrgChangeRecord.ready` and `provisioning_failed.provider_agent_id` (both optional on read).
  - **For Codex (reserved path):** `genesis_providers::ProvisionRequest::config_digest()` still hashes `Value::to_string()`; switch it to `genesis_core::digest::canonical_sha256`. Known limitation: a retried submission and restart recovery can drive the same change concurrently; add a per-change in-flight guard before live providers are wired.
- **Checks run:** after merging the PR #3 fixes into `feat/planner`, see the merge commit message for counts. At `72cb13e` (`feat/kernel-ui`): `cargo test --workspace` 199 passed, also with `--features serde_json/preserve_order`; clippy `-D warnings` clean; web vitest 16 passed. Planner tests use a scripted replay model against a real in-process kernel. No live LLM or sponsor calls yet.
- **Next implementation steps:**
  1. Pick the planner model and run the venue scenario live; add a second, different disruption to show a different response.
  2. Implement `CapabilityExecutor` with `genesis-apify` and use `genesis-verification` evidence for publication and Org B acceptance.
  3. Elastic `LlmClient` / `RoleProvisioner`, CREAO and Fetch adapters as those workstreams land.
- **Unpushed work or recovery notes:** Start with `docs/DEV.md` and `docs/PLANNER.md`. Planner entry points: `crates/genesis-planner/src/agent.rs` and `prompt.rs`.

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
