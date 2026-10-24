# Implementation status

## Active repository checkpoint

Work and reviews belong in [genesis-history-experiment](https://github.com/HenryWashuHe/genesis-history-experiment)
and [issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1).
Use the [migration map](REPOSITORY_MIGRATION.md) for legacy PRs. Initial main
`e1f2264` matches old `8794e25` by complete tree; rewritten commit identities
are preserved. The [ongoing goal](ONGOING_GOAL.md) remains open: a bounded
A → B → A executable development-method loop and honest controlled evaluation
with fixed model weights. Event organization remains superseded.

At main `df945c9`, these reviewed increments are merged:

| Work | Validation and evidence boundary |
| --- | --- |
| Method execution #18 (`07961f1`) and independent scoring #22 (`7510b86`) | Composition `785a22c` / `4d64fe6` passed 102 core tests and strict Clippy; merged tree matches `6742948e`. Pure experimental records, not executor/evaluator runtime. |
| Trial plan #28 (`e47ee64`, source `f52f182`) and experiment ledger #32 (`8578759`, source `0fa3773`) | Independent composition `d386fb0` / tree `680cf27e` passed 116 core tests and strict Clippy. Frozen-plan records retain all declared attempts; the ledger binds cumulative public experiments, unknown/overrun evidence and exact sealed transcripts. |
| Method source packages #36 (`2b2ecac`, source `f856a5b`) and independent probes #40 (`df945c9`, source `78dde86`) | Reviewed composition `a735bc7` / tree `0125f83c`: 123 core tests plus two independent multi-case probes pass (125 total), strict Clippy. Received-byte identity, host-selected runtime/protocol, ordered dependencies and bounded source loading; no execution or adoption. |
| Adoption outcomes #11 (`eefde296`, source `7f2102f`) and refusals #12 (`7010866`, source `9eaf8e0`) | 70 focused tests and strict Clippy pass, preserving original independent regressions. Local refusal no longer claims a confirmed provider failure. Existing #8/#10 adoption foundations remain merged. |
| Stage #20 (`7023077`, source `bfe10d9`) | Independent composition `7508c5b` passed 15 VM tests, preserving both independent test files. Neutral startup, explicit replay opt-in, ordered live responses and exact manifest identity are checked. No browser walkthrough or sponsor evidence. |
| Evaluator proposal #13 (`728d42c`, source `1a7f542`) | Corrected protocol clauses are agreed: immutable commitment, unambiguous encoding, public comparison development, frozen normalized outputs before private scoring/reveal, exact evaluator identity. This is a reviewed proposal, not approved runtime or G0 completion. |
| CREAO v2 #25 (`0d6d96d`, source `d3434e5`) and protocol evidence #30 (`67340ba`, source `0311f62`) | #25: 20 independent focused tests and strict Clippy, original `affdaf4` regression unchanged. #30: five CLI and nine Node gate tests plus scoped strict Clippy. Saved protocol controls durable routing/hash and evidence labels; offline evidence only. |
| Apify rereads #26 (`f153f58`, source `46ada41`) and independent regressions #29 (`b69f2ba`, source `041d478`) | Composition `601e381` passed 44 package tests and three additional visibility/identity cases plus strict Clippy; #29 preserves those cases, rerun on `d81d618`. #34's later read-window correction and live boundary are below. |

The #28/#32 libraries do not enforce prior freezing, exclusive durable state,
anti-rollback, actual metering, process isolation, wall/model limits or reveal
timing. Trusted executor/evaluator integration remains necessary. Their merge
does not establish affected-owner agreement on the complete runtime or close G0.

Henry [acknowledged and started the evaluator/executor lane](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6381504911).
The split evaluator PRs #37 `53c4473`, #38 `b141b89` and #39 `24b1ecd`
were reviewed on composition `2415024`. All 13 original tests and strict Clippy
pass, but independent geometry regressions `68e8857` reject separated circles
and accept off-body anchors; weld regression `577e928` shows initial relative
rotation erased in the first step. Motor torque also lacks the promised ceiling.
#37/#38 remain held. #39 fixes source-content hashing, reservation-only usage
and unknown consumption in its own delta, but depends on those fixes; it is
explicitly simulation primitives, not the trusted scoring adapter.

Neither simulator nor executor is on main. Henry's executor isolation design is
still a proposal pending independent checks; the handwritten seed is a control,
not a generated method. The merged source loader is available for executor
integration without selecting its ABI or resolving dependency bytes. A loaded
artifact is not execution, local acceptance or method-derivation proof.

Henry retains adoption proposal #14 `c67a324`: its control-free acceptance option
conflicts with TrialPlan v1's required candidate/control pair, and its receipt
installation-ID claim conflicts with #18. Adoption UI #15 `87624fff` has three
P2 holds: historical snapshots reveal future outcomes, stale candidate responses
overwrite newer decisions, and registry absence receives an overbroad tooltip.
Ten original tests pass; three independent `ae6ba16` cases fail. Eric's separate
invention-lab UI assignment remains unacknowledged; Ines is unassigned.

ACP #3 `3ad7685` fixes fresh-journal sender identity and confirmed-POST wording;
138 existing tests pass in composition `c099fdd`, but upgraded journals retain
inline `UNIQUE(message_id)` and fail preserved independent `bb5a841`. #5 `fc956a8`
clears its exact-ID/size and overflow-fixture holds but depends on #3's migration.
#4 still needs routing/status corrections. #6's migration script was withdrawn,
not approved; venue fallback #2 was closed unmerged as superseded, with its branch
preserved. Current method-development work does not depend on that venue scope.

[Issue #1's live Apify report](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6380843098)
records a successful Actor build and `SUCCEEDED` run. A later exact dataset GET
held one item whose venue fields matched expected values in a direct offline
comparison excluding per-venue evidence. Forge credited 0/3 cases after immediate
collection returned `InvalidDataset`; the gate is **failed**. That private cloud
checkpoint was not independently inspected here. A [later rerun](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6381127464)
after #26 again failed collection; its report places the final read about 1.3 s
after run completion, with the item readable later. #34 (`0425f8f`, merged
`0b4c18b`) now allows sixteen reads one second apart within the original deadline.
The new sixth-read regression fails on the old code and passes after correction;
187 Apify/forge test executions pass (including repeated harness cases), with
strict Clippy and formatting. This remains offline mitigation, not a confirmed
live resolution or verified capability. CREAO v2 metadata authenticated but returned no hosted agent; no
v2 agent/run has completed through the merged adapter. Fetch discovery has
partial authenticated evidence; Elastic index permissions remain unresolved.
See [live gates](LIVE_GATES.md) for these separate evidence categories.

Earlier delivery foundations remain: #16 path/redaction tooling (24 offline
tests), #17 routing (140 link/anchor checks), #9 constitution audit (18 scoped
independent tests), and #21 Actor metadata packaging (149 author tests and 58
independent focused tests; merged tree matches reviewed `be312625`). These
source-specific counts overlap and must not be summed into a test run of main.
Repair WIP is isolated in draft #7; its original files remain untouched.
Current [ownership and next work](AGENT_HANDOFF.md#current-ownership-and-next-work)
and the [goal ledger](ONGOING_GOAL.md#current-milestone-ledger) guide the next step.

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
