# Implementation status

## Active repository checkpoint

Work and reviews belong in [genesis-history-experiment](https://github.com/HenryWashuHe/genesis-history-experiment)
and [issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1).
Use the [migration map](REPOSITORY_MIGRATION.md) for legacy PRs. Initial main
`e1f2264` matches old `8794e25` by complete tree; rewritten commit identities
are preserved. The [ongoing goal](ONGOING_GOAL.md) remains open: a bounded
A → B → A executable development-method loop and honest controlled evaluation
with fixed model weights. Event organization remains superseded.

At main `67340ba`, the following new contracts and provider corrections are merged:

| Work | Validation and evidence boundary |
| --- | --- |
| Method execution records #18, merge `07961f1`; independent scoring records #22, merge `7510b86` | Tested composition `785a22c` / `4d64fe6` passed 102 core tests and strict Clippy; merged tree matches `6742948e`. Pure experimental records, with affected-owner feedback; no executor or evaluator runtime. |
| Opt-in CREAO v2 #25, merge `0d6d96d`, source `d3434e5` | Saved protocol version controls routing, durable fencing and specialist request hashing. Independent composition `e977ab4` passed 20 focused tests and strict package Clippy; original regression file `affdaf4` is preserved byte-for-byte. Offline loopback evidence only. |
| Apify bounded empty-page reread #26, merge `f153f58`, source `46ada41` | At most five GETs after a pinned successful run, within the operation deadline; no extra Actor POST. Independent composition `601e381` passed 44 package tests plus three extra visibility cases, strict Clippy and formatting. |
| Independent Apify regressions #29, merge `b69f2ba`, source `041d478`, composition `d81d618` | Three new tests pass on the composition with main `0d6d96d`: fifth-read success, immediate failure for seven invalid-page variants, and no dataset collection for four substituted run-identity fields. These do not prove live resolution. |
| CREAO protocol evidence #30, merge `67340ba`, source `0311f62` | Saved protocol appears in CLI/gate summaries; invalid version is rejected before checkpoint preparation. Five CLI and nine Node gate tests plus scoped strict Clippy pass independently. Offline evidence only. |

Draft [#28](https://github.com/HenryWashuHe/genesis-history-experiment/pull/28)
at `f52f182` adds a frozen pre-execution trial plan and assessment retaining
no-output, failed, interrupted, missing and unknown attempts. It pins criterion,
public instances, hidden commitment/protocol, exact methods, stable comparison
arms, local candidate/control acceptance and equal grants. 109 core tests pass;
seven new cases were independently rerun, and strict Clippy/formatting pass.
Affected-owner interface feedback is pending. This does not enforce prior
freezing, process isolation, cumulative experiment budgets, reveal timing or
authentication. Trusted runtime integration for the executor, experiment transcript
and evaluator remains necessary; no G0 completion or comparative result is claimed.

Draft [#32](https://github.com/HenryWashuHe/genesis-history-experiment/pull/32)
`0fa3773`, stacked on #28, now implements the library experiment-call ledger.
It admits exact public-instance/design calls under one cumulative simulation
grant, checks an independently supplied expected slot, retains charged failures,
unknowns and overruns, and seals exact transcript bytes for receipt validation.
116 core tests and strict Clippy pass; two independent probes check coordinated
slot substitution and preservation after invalid/duplicate settlement. Runtime
exclusive persistence, anti-rollback, actual metering, sandboxing and wall/model
limits are still absent from this module. Both drafts await affected-owner
feedback; no executor, authenticated execution or improvement result follows.

Henry's core Claude acknowledged the evaluator lane and pushed an isolated
implementation spike. Reviewed `b065a70` passes seven existing tests independently;
newer `d65cf3e` is reported but not independently reviewed. Neither is on main.
The reviewed spike hashes version labels instead of complete
evaluator content, accepts a fresh caller-supplied remaining budget on each call,
and lacks the merged scoring contract's trusted adapter. Two CLI experiments
each accepted the same 1,200-step allowance; this reproduces missing cumulative
enforcement, not a method improvement. Revised proposal #13 `f3b6a12` fixes the
main holdout timeline and content-identity requirement but retains contradictory
reveal/experiment clauses and ambiguous commitment encoding. #14 `9cd04e1`
fixes evaluator identity/routing but still blurs development with hidden scoring
and overstates unrelated-task non-use. Both remain proposals under review.
Henry's proposed narrow executor subtree is now explicitly assigned in
[issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6381016259),
awaiting acknowledgment; its handwritten seed will be labeled a control. Codex
retains the shared call/transcript contract. Assignment does not imply execution.

Adoption foundations #8 and #10 are merged (`8f26401e`, `1aed609f`), with the
same tree as tested composition `015e4fc5`: 47 adoption and 17 provider-executor
tests plus strict Clippy. #11 `7438308` now avoids invented discovery cardinality,
but still labels local pre-submission refusal as a confirmed provider failure.
All 56 existing gap-adoption and eight capability-adoption tests pass; new
independent `d11a8ba` fails with zero provider GETs/POSTs. #11 remains held;
#12/#15 are unmerged dependents. Method-adoption wiring remains separate.

ACP #3 `3ad7685` fixes fresh-journal sender identity and confirmed-POST wording;
138 existing tests pass in composition `c099fdd`. Existing journals still retain
inline `UNIQUE(message_id)`; independent `bb5a841` is preserved and fails.
#5 `fc956a8` fixes exact IDs/size and the overflow fixture; its focused overflow
test was independently rerun successfully. It still depends on #3's migration.
#4 needs current routing/status language. #6 was withdrawn and closed as
superseded by #17; its unsafe migration script was not approved.

Stage #20 `b91b9a4` now labels its storyboard and pages beyond the first 1,000
events. Six author tests pass; independent `d7e06e7` preserves two passes and
four failures: delayed replay callbacks overwrite live mode, out-of-order polls
regress state/duplicate events, inherited source digests are not matched, and
the same organization can be presented as independent inheritance. Its primary
event-organizer story is also superseded. ACP retains `apps/stage/**`; Eric's
separate invention-lab UI assignment remains unacknowledged. Ines is unassigned.

[Issue #1's live Apify report](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6380843098)
records a successful Actor build and `SUCCEEDED` run. A later exact dataset GET
held one item whose venue fields matched expected values in a direct offline
comparison excluding per-venue evidence. Forge credited 0/3 cases after immediate
collection returned `InvalidDataset`; the gate is **failed**. That private cloud
checkpoint was not independently inspected here. The merged reread is an
offline-tested mitigation; no live rerun confirms the cause or verifies a
capability. CREAO v2 metadata authenticated but returned no hosted agent; no
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
