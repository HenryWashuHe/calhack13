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

- **State:** Claude owns kernel; fallback authorized, not activated.
- **Kernel branch / latest commit:** To be recorded by the kernel owner.
- **Files currently being changed:** To be recorded by the kernel owner.
- **Completed behavior:** To be recorded by the kernel owner.
- **Checks run and failures:** To be recorded by the kernel owner.
- **Next implementation steps:** To be recorded by the kernel owner.
- **Unpushed work or recovery notes:** To be recorded by the kernel owner.

## Available capability implementation

Branch: `codex/capability-verification`; implementation checkpoint: `6ea6e3d`.
The [integration guide](VERIFICATION_INTEGRATION.md) describes the Rust APIs,
oracle/receipt boundaries and persistable Apify run tickets. Run
`bash scripts/check-capability-core.sh` for local validation. The implementation
passes 72 tests and one compiled documentation example, plus formatting and
Clippy. Live Apify smoke testing is still pending; hosted CI needs workflow-write
permission. Kernel persistence and organization authority remain kernel work.
