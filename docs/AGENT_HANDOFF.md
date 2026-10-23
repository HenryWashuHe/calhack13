# GENESIS implementation handoff

Updated: October 21, 2026.

## Current ownership

| Area | Current owner | Status |
| --- | --- | --- |
| Organization kernel, shared runtime contracts, database and application API | Claude | Assigned; no takeover recorded |
| UI | Claude | Assigned; unchanged by the kernel fallback |
| `crates/genesis-verification`, `crates/genesis-apify` and their checks | Codex | Implemented and pushed in [PR #1](https://github.com/HenryWashuHe/genesis/pull/1) |

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
- **Checks run and failures:** `cargo test --workspace` 134 passed (kernel 49, planner 12, capability 73); `cargo clippy --workspace --all-targets -- -D warnings` clean; web vitest 7 passed. Planner tests use a scripted replay model against a real in-process kernel. No live LLM or sponsor calls yet.
- **Next implementation steps:**
  1. Pick the planner model and run the venue scenario live; add a second, different disruption to show a different response.
  2. Implement `CapabilityExecutor` with `genesis-apify` and use `genesis-verification` evidence for publication and Org B acceptance.
  3. Elastic `LlmClient` / `RoleProvisioner`, CREAO and Fetch adapters as those workstreams land.
- **Unpushed work or recovery notes:** Start with `docs/DEV.md` and `docs/PLANNER.md`. Planner entry points: `crates/genesis-planner/src/agent.rs` and `prompt.rs`.

## Available capability implementation

Branch: `codex/capability-verification`; implementation checkpoint: `6ea6e3d`.
The [integration guide](VERIFICATION_INTEGRATION.md) describes the Rust APIs,
oracle/receipt boundaries and persistable Apify run tickets. Run
`bash scripts/check-capability-core.sh` for local validation. The implementation
passes 72 tests and one compiled documentation example, plus formatting and
Clippy. Live Apify smoke testing is still pending; hosted CI needs workflow-write
permission. Kernel persistence and organization authority remain kernel work.
