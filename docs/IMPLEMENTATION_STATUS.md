# Implementation status

## Active repository checkpoint

Work and reviews belong in [genesis-history-experiment](https://github.com/HenryWashuHe/genesis-history-experiment)
and [issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1).
Use the [migration map](REPOSITORY_MIGRATION.md) for legacy PRs. The
[ongoing goal](ONGOING_GOAL.md) remains a bounded A → B → A executable
development-method loop and honest controlled evaluation with fixed model
weights. Event organization remains superseded. G0–G4 are still open.

Main through `c7027a4014fba3e5c2be942165f98c487f5c9d6a` includes the reviewed
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
| Generation guards #63 (`46fd572`, source `60ac021baff05fd5fb5a8c70d33bd64c204ce297`) | Seven new cases and full 144-test serial forge run independently pass; strict Clippy/fmt. Henry's test files are unchanged. Mutation campaign remains author-reported. Initial parallel setup failures are recorded in #67 below, not attributed to these guards. |
| Parallel CLI fixtures #67 (`14fbdc1777b5df169fbccc6c2a4c5e7ca58e811e`, source `0b861365f715c1312eea0ec70f1244dae025e6ab`) | Test-only counters prevent same-process PID/time directory collisions. Original accounting run stopped at 86 passed/1 failed; accounting-only correction then exposed generation fixture collision at 95/1. Final default-parallel forge run: 144 passed, zero failed/ignored, strict all-target Clippy/fmt. Counts overlap earlier runs. |
| Parent-bound revision #66 (`22e73fa3e90fab0bee3ae4b2d6591fe4f924096a`, source `19f2a24036f2e1cc10845a57ae6aa470ba950ad5`) | Source tree `93172bd780be5901681e1ae2e5cad0f2e5ab3c82`: seven focused revision cases independently pass; author full 134 core tests and strict Clippy/fmt. Exact source inspection without execution authority, frozen parent input and whole-plan matching; source relabeling alone is not a change. Synthetic completion only, not HTTP/model/execution or recursive improvement. [Contract](invention-lab/contracts/revision.md). |
| Revision CLI #68 source `49c14958751a4f0fc335623d507ca74a7474381c`, merge `9c3633d69b4ac5fa618b083252542779c93d0d74` | Author full default-parallel forge: 156 passed, zero failed/ignored across 22 targets; five new CLI cases independently pass with scoped strict Clippy. [`method-revise-prepare` / `method-revise-observe`](invention-lab/contracts/revision.md#cli) expose exclusive offline preparation and read-only source derivation. No model call or source execution; overlapping counts. |
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
scoring primitives. The merged acceptance adapter checks scoring grants and exact
request/execution/output identity; hidden comparison closure remains held in
#57. The source and ledger libraries cannot enforce durable exclusive state, anti-rollback, metering,
process isolation, wall/model limits or reveal timing on their own.

The merged #42/#43 foundations correct cooperative exclusive admission, false
zero-model accounting, receipt-before-durable-ledger ordering and post-seal
submission. Model use is now Unknown in every sandbox mode. Best-effort mount
restrictions and trusted-directory storage do not establish safe general
untrusted execution; no Linux namespace enforcement is independently proved here.

Process runtime/CLI and acceptance scorer/verdict are now merged: #44 source
`3040ffaab2bc59aee02c46a08b4659f9a4d864f0` →
`0604664b33c6c1439e64075b140eeb5cb1431038`; #45 source
`2a83d779d9ee49ff9374c1fd61c7f2569c641e0a` →
`07756f3e1b68be827a9f1c72f4f54eb0beb8f8f6`; #46 source
`3fcbdf74e5b4c939e80b95ef8f1310895f0cf61c` →
`2deac69e0aa1d82bc986645e0a7a33b040a4e189`; #48 source
`25b4e7419b612da9144c8e2b6f7ae74c070ddfb2` →
`194c3ff496631a40e77316779db2d4a286f77df7`. Merged tree
`3cf1b091c7eb22b43c360ff2332725aea735cda3` matches independently reviewed
composition `3f8658c05bb50f6ca54531915849d84ae371d7b8`, without #57 refactors.
Executor tests exercise 19 portable cases; one namespace function self-skips
(reported harness total 20). Scoring has 16 passes and one ignored withdrawn
policy probe. Original `8163e3f` review files are byte-identical, both request
and reply pipe probes pass, and strict all-target Clippy passes for both
standalone crates. These checks clear the earlier request-queue and verdict
precedence holds, not Linux isolation or method-loop gates.

Hidden scoring #57 `9385fae83117de484908d1ae4b7e551e1e405290` clears the original
recovery/process-closure regression but has a new independently reproduced hold.
Test-only `1b61ffbaaca3024adac016fda0ca01bedaa27f85`, tree
`5b285be2bedf756e4e30cf4b0bc1e3bca901a4c7`, adds the bounded actual-CLI
`review_hidden_descendants.py` probe while preserving four earlier review files
byte-for-byte. Hidden scoring returns Scored after the method parent exits while
its forked child's heartbeat continues. Henry must establish termination of the
whole candidate process tree before hidden evaluation, not only parent exit.
This is local process evidence, not a Linux namespace test. See the
[exact-head review](https://github.com/HenryWashuHe/genesis-history-experiment/pull/57#pullrequestreview-5637952020).

Isolation proposal #65 `b8fff176ec877de93a7836ecccefdb6699ae40b3` is held on a
portable-test compile failure: unconditional `std::os::linux` imports produce
E0433 on macOS. The narrower image is a source improvement, not Linux isolation
certification. See the [review](https://github.com/HenryWashuHe/genesis-history-experiment/pull/65#pullrequestreview-5637952059).

Method-adoption #64 `b9d1f19886f0f1368397a0c45ce8fb0820aeb8fc` remains held on
[its independent binding review](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6383158573).
Unchanged test-only `005585a79d54f9fe09c0259fb2e72edc992e23d3` is preserved in
review commit `4359117ea296771979750891f63928ee991a84da`: two author tests and
two independent controls pass, but two independent cases still conclude Adopted
when the receiver rejects the exact runtime or already has that method installed.
A 16-process × 8-round stress control passes; the unclaimed CLI exists/write
sequence remains a source concern, not a reproduced overwrite. Henry retains
this offer/receiver-to-candidate binding correction. These are local fixed-design
and process fixtures, not G2 completion.

Prior runtime/scoring evidence remains in the [handoff](AGENT_HANDOFF.md#evidence-for-this-checkpoint).
Confirmed failed execution with scoreable output remains allowed. Scored output
with measured overrun is retained evidence that fails budget validation; wall
checking remains episode-granular, not hard preemption.

Merged #61 source `fb522c94ee945b9266eef37f9c89084b25a319af` →
`c7027a4014fba3e5c2be942165f98c487f5c9d6a` was rerun on final composition
`4f50e3f0bcaddb86671ffe069bc62b8194a9c72d`. Its tree
`45ef6d6dc928fa1aa84411d57471cae7283d8238` matches main. The focused rehearsal
passes 1/1 in 4.97 s with zero ignored cases: **Rejected, candidate 0/3 versus
control 0/3**, with direct library cost assertions passing. Its generation
roundtrip is in-memory; retained-file cost CLI integration remains Henry's lane.
Handwritten source, loopback fixture transport and a local `Mode::None` child
with explicitly permitted unknown model usage do not establish actual model
generation, independent method acceptance or recursive benefit.

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
verdict. Provider/settings reproducibility and the controlled comparison remain open. The merged
[offline read-only CLI](invention-lab/contracts/accounting-cli.md) exposes that
assessment without checkpoint mutation or provider calls. Omitted declared
evidence stays unknown; a named missing/malformed/substituted file is an error.
Exit 0 means assessment completed and can include unknown usage or overruns,
not that a method was accepted or its budget was satisfied.

The merged [revision preparation API](invention-lab/contracts/revision.md)
adds a G3 prerequisite: canonical parent bytes become an exact frozen public
input to one ordinary generation attempt. A read-only observation validates the
whole saved plan and distinguishes source byte changes from metadata relabeling.
It does not authenticate org identity, prove parent adoption or show improved
behavior. Its tests construct synthetic completions without HTTP. The merged
[offline CLI](invention-lab/contracts/revision.md#cli) prepares the existing
checkpoint exclusively and observes exact source derivation without mutation.
These commands make no provider request and execute no source.

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
