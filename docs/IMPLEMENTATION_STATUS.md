# Implementation status

## Active repository checkpoint

Work and reviews belong in [genesis-history-experiment](https://github.com/HenryWashuHe/genesis-history-experiment)
and [issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1).
Use the [migration map](REPOSITORY_MIGRATION.md) for legacy PRs. The
[ongoing goal](ONGOING_GOAL.md) remains a bounded A → B → A executable
development-method loop and honest controlled evaluation with fixed model
weights. Event organization remains superseded. G0–G4 are still open.

Main through `6da4a9660acdf0424fa756ba410a1d9126ebb85a` includes the reviewed
foundations below. Method generation merged at `d1be8443fee46e1196861926a963563f48407936`,
whose tree `dc9d6adc19863d8584fd5af1fd11eae7552014eb` exactly matches the independently
validated generation stack. Counts refer to the named source/composition, not a
combined main run.

| Work | Validation and evidence boundary |
| --- | --- |
| Execution/scoring records #18/#22 (`07961f1` / `7510b86`) | Composition `785a22c` / `4d64fe6`: 102 core tests and strict Clippy. Experimental records, not an executor. |
| Trial plan #28 (`e47ee64`, source `f52f182`) and experiment ledger #32 (`8578759`, source `0fa3773`) | Composition `d386fb0` / tree `680cf27e`: 116 core tests and strict Clippy. Declared attempts, cumulative reservations/settlements, unknown usage and exact transcripts are represented. |
| Source packages #36 (`2b2ecac`, source `f856a5b`) and probes #40 (`df945c9`, source `78dde86`) | Composition `a735bc7` / tree `0125f83c`: 125 core tests and strict Clippy. Exact received bytes, ordered dependencies and host-selected runtime/protocol; no execution or adoption. |
| Simulator #37/#38/#39 (`dc574135` / `3da2d4f` / `965ffb34`; sources `858ca41` / `7a16825` / `5f1e151`) | Composition `949eb569440da6b4a81b5d1b2ca3fc7cc1774839`, tree `2596db96354f10ad0bc5df857c9303105a200bae`: 22 tests, strict all-target Clippy and fmt pass on ARM64 debug. Preserved geometry/weld regressions are unchanged and pass; full-shape bounds and the torque ceiling are checked. Local simulation primitives, not real engineering or cross-platform proof. |
| Executor foundations/session #42/#43 (`a76735d` / `58bcaef`; sources `638ac262` / `53054d50`) | Composition `a1e2afdce49294c7b962bdbbe96836e50d21982a`, tree `5e0243f2875a929548f8eafa2baf9209f1215380`: 19 portable tests executed; one Linux namespace test self-skipped on the nonroot Mac. Strict Clippy/fmt pass. Unchanged `b73b62d` finalization/sealed-submit probes and `3784da9` reply-backpressure pass. Experimental foundations only. |
| Evaluator/adoption proposals #13/#14 (`728d42c` / `407b692`; #14 source `505312ad7efcee57439efc6bf2f054c8899e0c73`) | Corrected holdout protocol, exact evaluator identity, required candidate/control acceptance and receipt boundaries. Reviewed proposals do not close runtime integration or G0. |
| Adoption outcomes/refusals #11/#12 (`eefde296` / `7010866`) and decision UI #15 (`b94af6d`, source `fd60f1a2a6de33daf30afc87f80544c1833681a9`) | #11/#12: 70 focused tests and strict Clippy. #15: 13 focused tests and typecheck, preserving `ae6ba16` unchanged. Local refusal wording and historical/asynchronous evidence-display regressions are corrected. |
| Method generation #50/#51/#52 (`3da52d2` / `1533e2b` / `d1be844`; sources `e5eadd4` / `bc8eca2` / `d6c331c`) | Exact merged tree above: 125 forge tests, strict all-target Clippy/fmt independently pass. Frozen complete request, durable intent before one POST, retained measured/unknown completion usage and exact package export. Legacy parser contracts stay unchanged; no live model call or method execution. |
| Core guard coverage #53 (`761eb2b`, source `c614bb13d03c2f9d8550358f86b1ba79683156e8`) | Henry's 173-line `eacfe0f` test file is preserved exactly with authorship (blob `c537e1e9755284ad9b51a79058e19f2ee469a197`). Reviewed composition passes 130 core tests and strict Clippy/fmt; five new cases independently pass separately. Mutation results are author-reported, not independently rerun. Test-only change. |
| Accounting identity #56 (`2b69c25`, source `245cbadef63cc3051ef71bc24554234534275f8e`) | Author-validated 134 core tests and strict Clippy/fmt; four new regressions independently pass. Identity-only checks permit retaining overruns without relaxing existing acceptance/budget validators or changing valid hashes; malformed usage cannot hide behind an early overrun. This is not total-cost integration. |
| Declared combined cost #58 (`04029c56`, source `b27ae8a1f504d53effef42d3c12b1141d3299577`) | Author full forge suite: 128 tests and strict Clippy/fmt. Three new accounting cases and source independently reviewed. Exact generation/checkpoint/trial links, unsuccessful or unused generation and combined known/unknown/overflow/overrun costs; trusted declared roster, not proof of exhaustive history. |
| Accounting boundaries/guide #59 (`2cc05a65`, source `458ceab337c351cca80de005a5c1e9a41429b7b3`) | 15 focused cases independently pass: three accounting, six boundaries and six generation; author strict Clippy/fmt. See [the accounting contract](invention-lab/contracts/accounting.md). Counts overlap the preceding full suite; no provider execution. |
| Offline accounting CLI #60 (`6da4a96`, source `19592bd5039bb18243abc2794d7a8e96ecb6cc39`) | Reviewed tree `1ca02117479f8e09dfc6c9f214372ff37df3df9c`: 18 focused tests (12 existing CLI, three generation CLI, three accounting CLI), strict all-target Clippy/fmt pass. A second reviewer reran the three committed tests and nine temporary input/path/immutability probes without failures; counts overlap. [Read-only usage guide](invention-lab/contracts/accounting-cli.md). |
| Stage #20 (`7023077`, source `bfe10d9`) | Composition `7508c5b`: 15 VM tests, preserving independent files. Neutral startup, explicit fixture replay, ordered live updates and exact manifests; no browser/cloud proof. |
| Apify metadata-lag reads #47 (`48858e6`, source `86a9894bd06ecf1b00ef7ce999a985d24ab8c7cc`) | Baseline lag regression fails `InvalidDataset`; prior full run has 14 forge, 34 HTTP and 37 overlapping review cases plus doc-test. Six focused lag cases including two new transitions and strict Clippy pass. Existing sixteen-read window, deadline and effect/identity fences remain. A later live pass is operator-reported below; its primary evidence was not independently inspected. |
| CREAO v2 #25 (`0d6d96d`) and protocol evidence #30 (`67340ba`) | 20 independent #25 tests; five CLI and nine Node #30 tests; scoped strict Clippy. Protocol-fenced routing/hash and saved evidence labels; no hosted v2 agent/run established. |

The simulator is now on main. It includes exact-shape validation, seeded families,
fixed-step physics, trace hashes, immutable hidden-set descriptors and stateless
scoring primitives. Its trusted adapter must still enforce scoring grants,
request/execution/output identity and comparison closure. The source and ledger
libraries cannot enforce durable exclusive state, anti-rollback, metering,
process isolation, wall/model limits or reveal timing on their own.

The merged #42/#43 foundations correct cooperative exclusive admission, false
zero-model accounting, receipt-before-durable-ledger ordering and post-seal
submission. Model use is now Unknown in every sandbox mode. Best-effort mount
restrictions and trusted-directory storage do not establish safe general
untrusted execution; no Linux namespace enforcement is independently proved here.

Henry's process runtime #44 `7f369d15` remains held: independent test-only
`4fbe283a124186efeae057212f2bb51d568bdded` sends at most roughly 32 MiB and shows
that inbound requests accumulate in an unbounded channel while replies are
blocked. A temporary bounded-queue change passes the probe; production was
restored. CLI #45 `7b76d32` inherits this hold. Its offline exact-protocol/runtime
forge preparation passes and rejects tampered protocol without a POST; it does
not establish generated execution.

Scorer #46 `1fe7d21` clears its own context, zero-grant, scoring-worker and measured
overrun findings, but still depends on the runtime stack. Verdict #48 `4f266181`
passes the corrected original probes; composition/test commit
`8163e3f24d2cc0568e0b66885a0e56c77d60ad27` reports 14 passing cases, two new failures
and one historical ignored policy probe. The two failures reject an Invalid
candidate before checking either candidate or control executor's Unknown outcome.
That violates the agreed any-Unknown precedence rule. Confirmed failed execution
with scoreable partial output remains allowed. Scored output with measured overrun
is valid retained evidence that fails budget validation; no requirement to
rewrite it as Failed is imposed. Wall checking remains episode-granular, without
hard per-episode preemption. The [review checkpoint](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6382612367)
records the exact boundaries.

Hidden-comparison scorer #57 `f377445` has its own reproduced hold. Test-only
`d7543ed643e0ca8991f46d27f91dbbfca8918272` (tree
`2fbae1a862d91ea332cc6919a23b9787e024cee8`) submits a design, kills the executor,
recovers an Unknown receipt, then observes hidden scoring return Scored while the
method heartbeat still advances. Nine existing focused tests and strict Clippy
pass; the new case fails and a separate rerun fails in 0.69 s. This is explicitly
`Mode::None` local-process evidence, not Linux namespace enforcement evidence.
The adopted contract requires every candidate to stop before hidden scoring;
Henry must retain the regression and provide termination evidence or fail closed
when stopping is unknown. Parent #48 remains separately held.

New routing heads #44 `1ec58b2`, #45 `38d9fcb`, #46 `b8da4f1`, #48 `d6f2140`
and #57 `16ebdee` merge main and move #45's synthesis-plan helper into the example
library. Source comparison finds `run.rs`, `verdict.rs` and `hidden.rs` unchanged;
#44/#48 failures remain tied to their original tested sources. #57 `16ebdee` was
freshly rerun with unchanged `d7543ed` in review commit
`e5f56b50f6556cb85e010baf21e4a1b596bc4ee7`, tree
`043bf4a76f99542a8e825a4a397708849238e385`: three original hidden-scoring cases
pass and the local `Mode::None` process-closure regression fails again. This is
not a full-stack rerun or Linux namespace evidence.

#61 `f57234a8e3adb7f6730a343eab9a64940f8779a9`, tree
`61a2d3938f106ccb103f8d508cb5569375447289`, clears its own functional review:
independent rehearsal 1/1 passes in 6.69 s, returning Rejected (candidate 0/3,
control 0/3, margin 1). It uses exact loopback wire/fixture transport, handwritten
control source and a local `Mode::None` child; the criterion explicitly allows
unknown model usage. This is not actual model generation, live provider execution
or benefit evidence. It stays unmerged on held #48. A minor wording correction
is requested because its in-memory JSON roundtrip does not prove durability;
that is covered separately by the existing CLI intent-before-POST test.

Author-reported executor runs use handwritten controls. No live generated
method, independently accepted method execution, A → B → A return or controlled
recursive benefit is established. Eric's separate invention-lab UI assignment
remains unacknowledged; Ines stays unassigned.

ACP #3 `3ad7685` retains the upgraded-journal hold: 138 existing tests pass, but
independent `bb5a841` fails on inline message-only uniqueness. #5 `fc956a8` clears
its own bounds/fixture findings and still depends on #3. #4 routing/status needs
correction. #6 was withdrawn; venue fallback #2 is closed unmerged as superseded.

The [latest Apify operator report](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6382245294)
records a passed live gate on main `2a070af`: three oracle-checked cases with the
handwritten baseline parser, Actor `n8kB1hNqgzkddvWKp`, build `D5hU7kSdSc20L08g3`.
Sanitized evidence files and the private checkpoint were unavailable for
independent inspection here, so this remains **operator-reported live evidence**.
It is a new operation after #47, not a retroactive pass of the earlier failed
checkpoint. It establishes no model generation or development-method result.
CREAO hosted execution, Fetch public use and Elastic permissions retain their
separate [live gates](LIVE_GATES.md).

Codex's merged [method-generation path](invention-lab/contracts/generation.md)
is available for scoped integration with Henry's runtime. It saves the complete
historical request, reconciles an interrupted request before reading credentials
and never automatically resubmits an uncertain attempt. Its current
`provider_default` sampling policy is not a fixed-settings G4 result. Generation
costs can now be combined by the merged [declared-roster accounting library](invention-lab/contracts/accounting.md),
including failed/missing generation with no trial slot. The host still declares
history and inherited-cost inclusion; the library does not establish roster
completeness, budget admission/enforcement, authenticated metering or an acceptance
verdict. Explicit sampling and the controlled comparison remain open. The merged
[offline read-only CLI](invention-lab/contracts/accounting-cli.md) exposes that
assessment without checkpoint mutation or provider calls. Omitted declared
evidence stays unknown; a named missing/malformed/substituted file is an error.
Exit 0 means assessment completed and can include unknown usage or overruns,
not that a method was accepted or its budget was satisfied.

Local `.env` configuration is pending: the required model selection/key
are absent and user input has been requested. Continue offline core work; do not
commit credentials or turn missing configuration into a successful live claim.
Earlier delivery/source-specific evidence remains in the handoff; overlapping
test counts must not be summed. Repair WIP is isolated in draft #7 and its
original files remain untouched. Follow current
[ownership](AGENT_HANDOFF.md#current-ownership-and-next-work) and the
[goal ledger](ONGOING_GOAL.md#current-milestone-ledger) for the next step.

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
