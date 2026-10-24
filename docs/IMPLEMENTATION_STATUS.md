# Implementation status

## Active repository checkpoint

Work and reviews now belong in [genesis-history-experiment](https://github.com/HenryWashuHe/genesis-history-experiment)
and [issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1).
The [migration map](REPOSITORY_MIGRATION.md) maps all current PRs and preserved WIP.
Initial new main `e1f2264` matches old main `8794e25` by complete tree; migration
is not implementation or fresh validation. Adopted method execution, simulator
and comparative benefit remain open under [the ongoing goal](ONGOING_GOAL.md).

Repair WIP now exists as isolated draft #7 (`110a107`), with the original twelve
files preserved byte for byte and full validation still incomplete. New #13/#14
are evaluator/adoption proposals. New #3/#5 ACP heads are advancing after review;
check current heads rather than reusing earlier passing counts as new evidence.
Apify and CREAO v2 metadata reads authenticated, but no hosted CREAO agent was
returned and the current v1 adapter needs a separately scoped compatibility path.
This is not full live-provider execution. See current ownership in the handoff.

Earlier merged checkpoint `f8bffa26` includes the new routing docs (#17),
live-gate path/redaction fixes (#16, 24 offline tests) and constitution transition
audit (#9, 18 independent scoped tests). Draft [#18](https://github.com/HenryWashuHe/genesis-history-experiment/pull/18)
contains proposed pure method identity/run-binding/receipt validators, source
`40e481e` and tested composition `1d84587e` (93 core tests; strict Clippy).
It remains **unmerged and experimental**, with affected-owner agreement pending.
It does not implement method execution, independent scoring, adoption or comparison.
ACP review advanced: #3 `3ad7685` clears fresh-journal sender identity and
post-201 wording; its existing suite passes 138/138 in composition `c099fdd`.
Upgraded journals still retain the old inline `UNIQUE(message_id)` constraint;
independent test-only `bb5a841` is pushed for the author. #5 `15d4cad` clears the
exact-command-ID and reply-size defects, but its new overflow test fails because
its fixture fits without omission. Composition `fce5c04` passes 149/151 tests;
the other failure is the inherited journal migration case. All seven original
regressions and both mutation files remain unchanged. Both PRs remain unmerged.

PR #6 is closed as superseded by #17: its unsafe migration script was withdrawn,
not fixed or tested, and main already contains fuller routing guidance. #4's
latest command proposal still needs current routing and precise implemented vs
pending-report semantics.
Later merged checkpoint `983f0443` adds adoption candidate evidence (#8,
`8f26401e`) and terminal-run retry (#10, `1aed609f`). Adoption merge `1aed609f` has the same tree as
independently tested composition `015e4fc5`: 47 adoption and 17 provider-executor
tests plus strict Clippy. Unknown/live effects remain blocked from retry.

Fresh Actor packaging (#21, source `d20cb5e`, reviewed head `7cc59ee`) now binds
Actor name/version before source hashing. Generated provenance v2 coexists with
exact read-only validation of saved v1 packages. Strict returned-source checks
and unresolved-effect recovery are unchanged. Three previous regressions were
reproduced; 149 author tests and 58 independent focused reruns passed. Main's tree
matches reviewed `be312625`. ACP reported a real Actor creation followed by name
normalization/source mismatch; this fix has no new live build/run evidence and
does not rewrite or resubmit that old checkpoint.

Draft [#22](https://github.com/HenryWashuHe/genesis-history-experiment/pull/22)
(`0871df6`, stacked on #18) adds pure independent scoring requests/receipts bound
to the entire execution, exact output and trusted scoring context. Its request
roster retains missing/invalid/failed/unknown outcomes and separate development
and scoring accounting. 102 core tests and strict Clippy passed; independent
review reran all nine new tests. This is **unmerged and experimental**. A roster
created after output selection cannot prove complete predeclared development
trials, and no evaluator, holdout isolation or comparative result is implemented.

The new #20 stage at `b468313` is unmerged. Exact-head source review and VM probes found replay/live mixing, overclaimed
installation inheritance and incomplete event paging; its default event-organizer
story is superseded. It must not replace the invention-lab direction or become
live-provider or method-improvement evidence.

The [goal ledger](ONGOING_GOAL.md#current-milestone-ledger) records next steps.

## Historical source checkpoint before migration


This checkpoint describes main `2e5d7f07784eff8563171fd34c85e79584b59fe7`
and separately identified work as of October 23, 2026 (Pacific time). Newer exact
heads and [issue #4](https://github.com/HenryWashuHe/genesis/issues/4) supersede
it. The [review queue](PR_REVIEW_QUEUE.md) records unresolved findings; the
[handoff](AGENT_HANDOFF.md) records ownership and authorization to continue.

GENESIS's direction is **networked recursive capability improvement with fixed
model weights**: organizations develop executable capabilities, selectively
inherit them, and improve the methods used to develop future capabilities.
Retaining a tool, transferring it, and improving the development process are
distinct claims. The [invention-lab demo](INVENTION_LAB_DEMO.md) is a proposed
way to test the larger claim; it is not an implemented simulator or a measured
result. Event organizing is no longer the product direction. Existing venue
fixtures remain regression evidence for the mechanisms they exercise.

## Merged implementation and its evidence

| Area | Present on the inspected main | Evidence boundary |
| --- | --- | --- |
| Organizational authority and persistence | Committed roles, responsibilities, assignment epochs, leases, capability attachment, retirement, event history and durable effect journals. See [core engine](../crates/genesis-core/src/engine.rs), [store](../crates/genesis-server/src/store.rs) and [saga](../crates/genesis-server/src/saga.rs). | Local invariant, integration and fault-injection checks exercise specific boundaries. Dynamic organization structure alone does not demonstrate better problem solving. |
| Generated task capabilities | Requirement/public-example synthesis, exact source/build binding, independent trusted-suite verification and one bounded repair of a rejected initial acquisition. See [forge acquisition](../crates/genesis-forge/src/acquisition.rs) and [server acquisition runtime](../crates/genesis-server/src/capability_acquisition_runtime.rs). | The generated runtime is specialized to TypeScript web extraction on Apify. Loopback clients/journals are exercised; canned model/provider output does not demonstrate that generated code works in a live provider. |
| Publication and independent inheritance | Registry publication/discovery, exact version/pin checks, receiving-organization acceptance, and retained capability use after the original specialist retires. See [registry](../crates/genesis-server/src/registry.rs), [adoption](../crates/genesis-server/src/capability_adoption.rs) and [two-organization tests](../crates/genesis-server/tests/two_org_inheritance.rs). | Fresh local checks in B are implemented. These fixtures establish bounded inheritance mechanics, not general applicability, measured improvement, authenticated shared repair advisories or automatic migration of installed versions. |
| Invocation evidence | Backend reconstruction of collected invocation evidence against exact artifact, input, suite and historical authority. See [invocation verification](../crates/genesis-server/src/capability_invocation.rs). | Semantic rejection is distinct from authority rejection and uncertain provider effects. The accepted suite defines the supported input scope. |
| Fetch transport and automatic recovery intake | Durable specialist contracts/messages, exact task receipts, verified recovery coordination and opt-in intake. See [Fetch crate](../crates/genesis-fetch/src/lib.rs), [intake](../crates/genesis-server/src/venue_recovery_intake.rs) and [native composition](../crates/genesis-server/tests/venue_assessments/recovery/native.rs). | #117's two explicit native tests use actual Python SDK processes and signed local transport. Apify, CREAO and initial provisioning remain fixtures. Intake is explicitly drained in the test; timer scheduling and public Agentverse/ASI execution are not established by it. |
| Operator console and ACP entry points | Organization/audit views and fixed recovery/mission adapters exist. See [web application](../apps/web/src), [recovery adapter](../services/fetch-bridge/genesis_fetch_bridge/acp_recovery.py) and [mission adapter](../services/fetch-bridge/genesis_fetch_bridge/acp_mission.py). | The mission ACP adapter merged with unresolved independent regressions. It is not a validated public end-to-end workflow. Invention-lab method lineage and comparative-evaluation panels are proposed. |

The merged scenarios use real server state transitions, adapters and verification
code with explicitly bounded fixture inputs. Some transport checks also use real
local SDK processes. Neither category establishes authenticated live cloud
execution, a complete autonomous mission, physical-world engineering results or
measured recursive improvement. Source-specific test counts in the review ledger
must not be summed into a claimed test run of current main.

## Paused, unmerged deployed repair

The `codex/deployed-capability-repair` branch preserves forge commit `1795fb7`
(the scoped source commit is `be2479a`). It adds a deployed-repair constructor
bound to the parent's current independently verified parser, an immutable origin
digest, one fresh replacement and unchanged-parser rejection. Its 19 acquisition
tests passed before the pause. This code is not on the inspected main.

Server repair creation/store/routes, 12 integration cases and draft repair docs
remain local uncommitted work. The complete failure → replacement → publication →
independent B acceptance happy path passed. The full revised 12-case target and
final checks had not run when work paused. No repair PR was opened or merged.
See the [preserved checkpoint and pause notice](https://github.com/HenryWashuHe/genesis/issues/4#issuecomment-6378835715).

That slice covers semantic failure on an already authorized frozen input. It
does not authorize unseen source layouts, revise historical acceptance evidence,
or implement cross-organization repair advisories. Preserving the work does not
make its old domain a requirement for the new invention-lab direction.

## Open review and known limitations

| Work | Inspected status |
| --- | --- |
| [#116 candidate decisions](https://github.com/HenryWashuHe/genesis/pull/116), correction `78a18c059e13dde3218da68615928bad05291b6f` | Correction pushed; independent re-review pending. The author reports all three prior P2 findings fixed, `4b44099` preserved, and 46 gap / 8 adoption / 5 scenario / 1 inheritance checks passing. The selection-order, unreadable-score and missing-conflict-log findings apply to reviewed `03f7101f`; they are not established findings against the new head. |
| [#120 constitution audit](https://github.com/HenryWashuHe/genesis/pull/120), `0cf383b7` | Open; root review/validation pending. Author-reported checks do not constitute an independent merge-ready decision. Eric retains its bounded audit scope. |
| [#121 terminal RunFailed](https://github.com/HenryWashuHe/genesis/pull/121), `5d711429` | Open; root review/validation pending. Proposed distinction between confirmed terminal provider failure and uncertain/live effects is not yet merged. Henry's core Claude retains this narrow provider/runtime interface change. |
| Merged [#114 mission ACP](https://github.com/HenryWashuHe/genesis/pull/114) | Seven independent regressions remain outstanding, including duplicate submission after restart, false no-change status and incomplete/misattributed follow-up outcomes. See [the current-main reproduction](https://github.com/HenryWashuHe/genesis/issues/4#issuecomment-6378650906). The ACP author/session remains separate from Henry's core adoption work. |

Henry's core Claude has also [claimed the next adoption outcome classifier](https://github.com/HenryWashuHe/genesis/issues/4#issuecomment-6379056143),
stacked on #116: `adopted`, `not_applicable`, `insufficient_evidence`,
`no_candidate`, `unknown`, plus pending/stopped states, with types/status,
focused classifier tests and docs. This is an active bounded claim, not a merged
interface. The conservative fallback contract remains unchanged. Coordinate
new method-adoption contracts with this owner rather than duplicating that work.

## Proposed, not implemented or demonstrated

- A generic executable development-method package and executor, with exact
  dependencies, bounded effects and independent acceptance. Workflow text in a
  manifest is not an executable development method.
- A visual physics invention lab, deterministic domain evaluators, independently
  controlled challenges and a working reciprocal A → B → A method-improvement
  demonstration. These require contracts beyond the current extraction runtime.
- Measured benefit from development-method inheritance over a strong equal-budget
  baseline, including failures, regressions, inappropriate transfer and the cost
  of generating and validating the shared method. A passing suite or growing
  artifact count is insufficient evidence.
- Authenticated network repair advisories, general method lineage, deprecation,
  rollback policy and independently accepted replacement selection across
  organizations. Existing installation pins must remain explicit.
- Public Agentverse/ASI operation and authenticated live sponsor execution that
  completes the intended workflow. No established full-mission evidence is
  claimed here.

The [recursive roadmap](RECURSIVE_CAPABILITY_IMPROVEMENT.md) and
[invention-lab proposal](INVENTION_LAB_DEMO.md) describe intended next work.
Ongoing work is resumed under the [repository goal](ONGOING_GOAL.md) and active
hourly continuation. The old app Goal card still needs lifecycle replacement;
that does not revive the venue objective or transfer colleague files. Local
deployed-repair WIP remains separately preserved. Ines is explicitly unassigned.
Consult the [current handoff](AGENT_HANDOFF.md) before taking any files, including
new contracts or simulator paths.
