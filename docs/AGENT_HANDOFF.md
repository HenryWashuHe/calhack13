# GENESIS implementation handoff

Updated: October 21, 2026.

## Current integration checkpoint

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
