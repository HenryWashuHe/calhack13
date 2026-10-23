# GENESIS implementation handoff

## Authoritative checkpoint — October 23, 2026

Source basis: main `9a9c7a0a459c929e3612dc942c5a8dcb9fd7d8bd`, tree
`05767561a3017163e9ff7af77de655070d6929f6`, after #82. This checkpoint
supersedes the historical status and ownership notes below; exact newer GitHub
heads and the scoped handoffs in [issue #4](https://github.com/HenryWashuHe/genesis/issues/4)
take precedence. The ongoing two-organization mission goal remains active.

**Merged:** #78 finalizer, #80 finalizer UI, #88 settled-check display,
#89 planner policy names/sources/scopes, #97 console guard tests, and
#90/#92/#93/#94/#95/#96 saga, audit and planner changes. The six-PR Rust
composition at `97d75eb` (tree `f305ca60`) passed **608 tests across 68 targets**,
scoped strict Clippy and formatting. These are source-specific results, not a
new aggregate count for every later main commit.

- **#98 Fetch float identity:** merged `9f9a87d`, source `11703e2`.
  Canonical JSON float identity survives journal/receipt reconstruction;
  stale changed intents cannot block release or unrelated healthy work.
  **133 Fetch/verification tests**, including ten float cases, pass. Three
  baseline defects and two release regressions were reproduced before fixes.
- **#99 monitor intake:** merged `72cd014`, source `05d55e2`, tree `5e5cf477`.
  Persisted scans choose the newest eligible source within their captured
  backlog, including across pages/restart. Manual consumption and temporary
  CREAO absence no longer permanently halt valid intake; exact child recovery,
  stop/expiry and authority fences remain. **34 assessment + 117 server-library
  tests**, scoped strict Clippy and formatting pass. Baseline evidence was four
  failing regressions plus four passing controls. See [monitor semantics](VENUE_MONITORS.md).

- **#82 adopt first:** merged `9a9c7a0`, source correction `ae00fa9`, tested
  composition `4c8c461` with the exact main tree above. The original same-owner
  delegation regression is unchanged and now passes. **41 focused tests**,
  scoped strict Clippy and formatting pass. The author branch
  `henry-claude/gap-adoption` was fast-forwarded to `4c8c461` to include current
  main; no history was rewritten. Claude should pull before continuing it.

**Open review queue:** #86 `83998b0` remains **held on a reproduced failure**.
A local check marked `Rejected` after suite drift can retain a native receipt
whose provider run is still `RUNNING`. Retry submits a second POST while an
independent GET confirms the first run remains active. Test-only `a497651` on
`codex/review-adoption86-rejected`, composed with current main at `c33c93f`,
reproduces this; it is loopback provider evidence, not a live sponsor run.
#91 `6776130` remains held on replay starvation under frequent nonstructural
events and missing revisions displayed as audited success. Draft #84 has a new
head `3d3219c`, **not yet reviewed**; the earlier preflight/coverage findings are
not cleared by its arrival. Test-only `4ed8c08` is preserved but not yet executed.
#83 `7c1900d`, #85 `c2abcf9` and #87 `88d9174` have clean scoped source reviews
and can proceed to current-main integration and execution independently of #86.
See [the exact queue](PR_REVIEW_QUEUE.md).

**Ownership:** Claude/Eric retains kernel, planner, readiness, adoption and UI,
including #86/#91 fixes. Claude's `henry-claude/requestable-adoption` claim covers
`crates/genesis-server/src/routes/capability_gaps.rs` requestable policy visibility,
`crates/genesis-planner/src/prompt.rs`, matching `RequestPolicy` / `Requestable`
deserialization and adjacent tests. The follow-up exposes adoption-only names
without acquisition sources/scope and uses the observation's public page URLs;
it does not change the coordinator, kernel or acquisition. See the
[scoped claim](https://github.com/HenryWashuHe/genesis/issues/4#issuecomment-6372394586).
Codex's monitor and Fetch float fixes are merged.
Codex continues the Fetch venue-recovery implementation at `299ee0c` on
`codex/fetch-venue-recovery`, with runtime/retirement integration tests still
pending; this is work in progress, not a merged mission result. Coordinate
before overlapping files. Preserve review evidence and push tested increments;
the active root reviewer sequences merges.

**Evidence and credentials:** no native sponsor run is established. Loopback
fixtures and local signed-process checks do not prove cloud execution or a
completed autonomous mission. The user has not supplied the required credentials:
`APIFY_TOKEN`, `CREAO_ACCOUNT_API_KEY`, `ELASTICSEARCH_URL`,
`ELASTIC_DATA_API_KEY`, `ELASTIC_KIBANA_URL`, `ELASTIC_MANAGEMENT_API_KEY`, and
`AGENTVERSE_API_KEY`. Setup also needs each provider's configured runtime and,
for synthesis/planning, the relevant model credentials. Follow
[LIVE_GATES.md](LIVE_GATES.md); preserve actual agent/run evidence and a
genuine CREAO Share link separately. Never put credentials in handoff files.

## Historical checkpoints

Everything below preserves earlier source-specific results and assignments.
Statements such as “current,” “next” or “hold” below describe their original
checkpoint, not the authoritative status above.

## Historical implementation and ownership — October 22, 2026

Immutable Fetch receipts are implemented at `afbe0bf` (reviewed original
`6424bfb`). Exact frozen recruitment/consent/task messages are checked against
prepared outbox and authenticated Applied inbox records in one SQLite snapshot.
Receipt hashes survive later work/release/restart; construction is private and
Debug hides payloads. **64 Fetch crate tests**, including 13 receipt tests, strict
crate all-target Clippy and workspace fmt pass. Independent review is clean.
See [the API and trust boundary](FETCH_RUNTIME.md#immutable-receipt-reconstruction).
This adds no server integration, native signature verification or domain verdict.

[PR #79](https://github.com/HenryWashuHe/genesis/pull/79) is merged as `1fc1b8e`,
reviewed source `135c839`, exact tree `c09245b`. Runtime `8741f38` registers one
durable subscription and observes both accepted assessment cycles. All 40 scenario
unit/HTTP checks and five full scenarios pass, with strict default workspace
all-target Clippy/fmt and clean independent review. Loopback evidence only.

Claude's [PR #78](https://github.com/HenryWashuHe/genesis/pull/78) is on review
hold at `7cb747c`. Composition `6be7dc2` on `codex/review-gap-finalizer78` preserves
five reproduced regressions and 11 passing controls: live requirement fencing,
weaker-suite acceptance, acceptance followed by delegation, and conflicting
same-pin publication. The ten original tests still pass. The two requirement
mutation tests use an explicitly documented Store commit seam; the acceptance,
delegation and publication paths use genuine public endpoints and loopback clients.
No production fix is included in that review branch.

Stacked [PR #80](https://github.com/HenryWashuHe/genesis/pull/80), `931192c`, also
awaits correction: global finalizer enablement with no per-gap record must not
claim work is queued. Its 66 original web tests/typecheck/build pass; `d369213`
adds the failing no-record regression and passing organization-switch/terminal
AttentionPanel control. Claude retains both implementations.

After those fixes, Claude is assigned the opt-in adopt-before-acquire gap
coordinator and narrow kernel transition in a separate PR. It must create one
durable adoption intent, retain uncertain outcomes, and permit synthesis fallback
only after a definitive persisted empty result. A correlation prefix cannot by
itself authorize a different selected capability ID. The detailed acceptance
criteria and ownership are in issue #4 comment `6354798168`; implementation has
not yet been reported.

Codex owns the immutable Fetch receipt slice, restricted to `genesis-fetch` and
its focused docs/tests. The implementation binds exact prepared requests to
authenticated Applied inbox evidence and preserves historical identity across
later tasks and release. No server bridge, kernel, planner,
readiness or UI changes are part of that slice. Native sponsor evidence remains
open. See the scoped handoff in issue #4 and the Fetch mission bridge plan.

[PR #77](https://github.com/HenryWashuHe/genesis/pull/77) adds opt-in durable venue
monitor subscriptions; **merged** as `1bb3c9f`, reviewed source `c889491`,
exact tree `3ad4418`. Runtime checkpoint `e699659` was based on main `3ea5ec4`.
One operator registration freezes the existing role/assignment/CREAO binding,
accepted capability and event policy. The server follows verified invocation
events, persists each exact child intent before creation, and uses the existing
assessment verifier for mission effects. Work is bounded by expiry and 1–16
intents. Unknown work retains its child identity; an accepted Reopen completes
the subscription with its replacement requirement, without completing the mission.
See [the API and limits](VENUE_MONITORS.md).

Validation: **154 source-scoped Rust checks** across four targets: 113 server
library, 14 CLI, 22 assessment integration (including nine subscription tests),
and five full scenarios. The original 21 assessment checks passed together; the
final test-only addition passed with all nine subscription checks. The same
runtime passed strict default workspace all-target Clippy and formatting.
The crash regression recovers a committed child after its link write fails,
including a stopped subscription, without changing the job/deadline/native run.
A 205-event regression preserves the cursor through restart, skips a different
event input and processes a job verified after registration. Providers are
loopback; the subscription tests use trusted synthetic provisioning/publication.
Native sponsor evidence remains open.

[PR #76](https://github.com/HenryWashuHe/genesis/pull/76) adds the offline monitoring
setup helper; merged as `3ea5ec4`, reviewed source `1ff0c94`, exact tree
`fe419e7`. Implementation checkpoint `bfcfe00` was based on main `64d6492`.
It validates three immutable portal namespaces and atomically emits B's frozen
baseline/update suite and the complete strict scenario. The explicit policy must
accept the baseline and reject only the update's capacity. Public aliases and
non-string identities are refused; template access flags are preserved.
All **35 portal tests pass without skips**, including the actual Rust verifier
and scenario CLI. Independent review is clean; the exact documented walkthrough
also passed with temporary paths and a fake HTTPS origin. See the
[executable setup](VENUE_PORTAL.md#prepare-continuing-creao-monitoring).
These checks make no native provider or unforeseen-update claim.

[PR #75](https://github.com/HenryWashuHe/genesis/pull/75) connects persistent CREAO
assessment to the two-organization scenario. Runtime checkpoint `f5136fe`, based
on reviewed main `8187430`, adds optional `venue_monitoring`: after independent
capability adoption and verifier retirement, the same newly created CREAO monitor
assesses a feasible baseline and a distinct capacity-cut snapshot. Its second
verified result creates one cited replacement requirement. Both proofs and the
same agent survive restart without extra native requests. See
[scenario setup and evidence limits](INHERITED_MISSION.md).

Validation: **145 default-feature Rust checks** across four targets: 113 server
library tests (including 37 scenario unit/HTTP checks), 14 CLI tests, 13 assessment
integration tests and all five full scenario tests. Strict default workspace
all-target Clippy and formatting pass. Independent regressions failed before and
passed after fixes for bounded local-verification recovery, the final permitted
poll, and the kernel's valid owner-only handoff with no separate executor.

All providers in these tests are loopback. The full scenario uses the real CREAO
creation/client/runtime/journal, with an explicitly labeled creation-journal
readiness fixture; it does not prove a live gateway callback or cloud execution.
Both B source cases are configured before adoption. Actual sponsor runs, a genuine
CREAO Share link and the complete autonomous mission remain open. The underlying
assessment API from merged #72 remains unchanged; see
[its runtime contract](VENUE_ASSESSMENTS.md).

Reviewed #70, #71, #73, #74 and #75 are merged. Main checkpoint `64d6492`
includes the persistent scenario and corrected audit. All three original audit
regressions from `c92f9d5` now pass unchanged: frozen gap binding, acquired pin /
publication stability, and consistent concurrent history reads. Six latest
history/regression tests and 37 scenario units pass; the prior composition's
identical core/CLI files passed 65 core and 14 CLI checks. Strict core/server
all-target Clippy and formatting pass. See [the review queue](PR_REVIEW_QUEUE.md).

Claude/Eric owns kernel/planner/readiness/UI and the explicitly assigned opt-in
gap `Acquired → Installed` finalizer. Codex owns the monitor subscription and
its scenario integration, along with the completed operator setup helper.
No audit review hold remains from the previous batch.
Coordinate through [issue #4](https://github.com/HenryWashuHe/genesis/issues/4).
The ongoing mission goal remains active; historical notes below do not supersede
this ownership or current GitHub heads.

## Previous merge and ownership checkpoint — October 22, 2026

Reviewed main checkpoint: `b2aebcd173972755297d68a1e161e2113cea5181`.
The capability stack, specialist execution, planner and Elastic integrations,
evidence and gap UI, portal holdout generation, gateway readiness verification,
operator attach, Attention UI, the durable gap driver and CREAO readiness probe
have landed. The reviewed queue is closed out. See
[the review queue](PR_REVIEW_QUEUE.md) for exact sources and checks.

Readiness #62 now fixes both fresh-schema recovery and legacy database migration;
it merged into #26 at `9935bd8`. Main's #26 integration passed 304 server tests.
The #27 composition passed 69 focused tests with `serde_json/preserve_order`
and strict default-feature Clippy. Main's #28 tree equals `0862df8`: clean
install, 56 web tests, typecheck and build pass, preserving the evidence-refresh
and gap-event tests. These are source-scoped checks, not a new full-workspace
total, native provider run or browser walkthrough.

The original #60 example-index mismatch is fixed and merged at `ec5ec22`.
`--holdout` supplies separate synthesis-example and verification pages; ordinary
portal generation still supports explicitly labeled reproducibility checks.
CREAO readiness probe #29 is merged at `ba4c920`, with 40 final default-feature
checks and strict workspace Clippy/fmt; its earlier scoped run passed 122 checks
with `serde_json/preserve_order`. The probe sends a durable challenge run, but
only a valid callback establishes readiness. Neither readiness nor operator
attachment establishes core mission work or a live sponsor demonstration.

Merged #66 implements the durable gap-to-acquisition driver. Initial source
`a66a298` passed 310 server tests across 30 suites, including 16 driver tests with
loopback provider responses and a real server-startup path. Final integration
`2561be` passed 152 default-feature tests across nine targets, strict workspace
Clippy and formatting; the later source delta was documentation only.
It freezes the request, model policy, suite and assignment, recovers one intent
across restart, and retains unknown-outcome and explicit read-retry protections.
`Acquired` records independent verification; publication, installation and
attachment remain separate. The next Codex core implementation is persistent
CREAO mission assessment backed by verified capability output and native execution
receipts. Readiness alone does not satisfy that work.

Combined draft #25 was closed without merging after all 17 combined-only merge
commits and remaining unique content were audited. No pending runtime was unique
to it. Its Elastic recording-proxy scenario
test was preserved in merged #68 (`2adaff4`): the targeted repaired A→B scenario
passed with real Rust Elastic retrieval/projection against loopback fixtures.
The old branch remains available; its 699-test total is historical. Merged #67
adds invariant tests: 63 core checks pass, including seeded sequences and six
sampled negative controls, with strict core Clippy/fmt. This is not an exhaustive
proof of every invariant.

Claude/Eric and Henry's Claude keep their scoped kernel/planner/readiness/UI work.
Codex handles capability/provider implementation, review and narrow integration;
no whole-kernel takeover is active. Coordinate in
[issue #4](https://github.com/HenryWashuHe/genesis/issues/4), push tested increments,
and leave parent/main merges to the active reviewer to avoid concurrent merge
races. Preserve source branches and exact-head evidence.

[CREAO sponsor evidence](CREAO_SPONSOR_NOTES.md) still needs core mission work,
an actual agent ID and a genuine run share link. The
[Apify notes](APIFY_SPONSOR_NOTES.md) retain meaningful-use requirements.
No live sponsor execution, public portal tunnel or independent-account Actor
access is established by these local tests. The ongoing mission goal remains active.

## Historical pre-merge checkpoint

The remaining sections preserve earlier checkpoints. References to “current”
below describe those earlier states, not the reviewed main checkpoint above.

Updated: October 22, 2026.

## Current integration checkpoint

Codex has pushed [PR #24](https://github.com/HenryWashuHe/genesis/pull/24),
`codex/automatic-adoption-scenario`, implementation `72157aa`, observer `cdfaf17`,
stacked on #19. Automatic mode sends one B-only adoption intent, observes durable
selection/acceptance, then compares the selected artifact with A's publication.
The report includes B's adoption ID/publisher; failed status reads retain the ID.
Manual mode remains compatible. See [INHERITED_MISSION.md](INHERITED_MISSION.md).

All **594 workspace tests** pass with `serde_json/preserve_order` (64 suites, zero
failed/ignored), plus formatting and strict default-feature all-target Clippy.
Five real-server scenario tests reject all legacy client search/install/checks/
acceptance and step/verify writes; acceptance survives verifier retirement and
SQLite restart, and successors still need fresh invocation proofs. Twelve pure
observer tests and eight HTTP driver tests cover binding, phase/version changes,
attention, malformed accepted status, polling bounds and failed observations.
All provider execution remains loopback-fixture evidence, not live sponsor proof.

Claude's updated #11/#12 and new #20–#23 are available. Original planner/Elastic
regressions pass; new planner recovery defects are reproduced in test-only
`70c4edb` and posted on #12. Specialist authority review is underway. Claude keeps
planner, general lifecycle/saga/outbox, role activation, Elastic and UI. Codex's
next scoped milestone is combined-stack integration in an isolated draft branch,
with registration/conflict resolution and integration checks. Known blockers
remain assigned to their source branches; no whole-kernel fallback or main merge
is implied. The ongoing implementation objective continues.

Capability dependency order: #8 → #9 → #10 → #13 → #14 → #15 → #16 → #17 → #18 → #19 → #24.

### Previous durable adoption checkpoint

Codex has pushed [PR #19](https://github.com/HenryWashuHe/genesis/pull/19),
`codex/durable-adoption`, core `a640b36`, terminal recovery `f346d6a`, stacked on #18.
B can create an adoption intent from its own requirement and trusted suite, discover
one compatible foreign capability through its configured registry, and commit its
own fresh local acceptance across restart. No A artifact ID is supplied, and B
needs no model client or acquisition configuration. Explicit role creation and
attachment remain organization-change operations.

The selected installation, complete check batch and final local acceptance each
commit atomically with the adoption stage. Child requests freeze adoption identity
and recheck current requirement, suite, assignment/epoch, constitution scopes and
deadline before/after provider work. Legacy endpoints cannot bypass adoption-owned
installations. Terminal attention/fencing closes an unaccepted installation using
an operational-stop reference, not a semantic verdict; child tickets and unknown
effects remain. A later explicit new intent is separate work, never automatic replay.
Registry transport is bounded, strict, redirect/retry-free and error-redacted.

All **584 workspace tests** pass with `serde_json/preserve_order` (64 suites, zero
failed/ignored), formatting and strict default-feature all-target Clippy pass.
Focused evidence includes ten store tests, eight actual-server adoption tests and
six registry-client tests. Integration proves independent B
inputs/suite, lost-reply replay, atomic rollback at four boundaries, unknown POST
preservation, authority/policy changes, verifier retirement, restart and successor
invocation. Provider replies remain canned loopback fixtures, not live sponsor
or cross-account access evidence. See [CAPABILITY_ADOPTION.md](CAPABILITY_ADOPTION.md).

Next Codex slice connects automatic scenario mode to B's adoption worker. Manual
mode remains compatible; automatic mode will observe the server decision rather
than send an A artifact ID or manage legacy acceptance steps. Claim paths in issue
#4 first. Claude retains planner/general lifecycle/saga/outbox, role activation,
Elastic and UI; #11/#12 heads remain unchanged. The overarching goal remains active.
Dependency order: #8 → #9 → #10 → #13 → #14 → #15 → #16 → #17 → #18 → #19.

### Previous acquisition history checkpoint

Codex has pushed [PR #18](https://github.com/HenryWashuHe/genesis/pull/18),
`codex/acquisition-history`, implementation `9c09aaf`, final regression `18d1d9b`,
stacked on #17. Operator-only acquisition listing and per-ID history support lost-ID
recovery and durable progress inspection after restart. Cursor pages are bounded;
mission/requirement filters match exact bounded UTF-8 identifiers. Snapshots omit
source, expected oracle data, provider bodies/native IDs, credentials and proofs.

Creation and checkpoint history share the acquisition/quota transaction; meaningful
attention changes share the scheduler transaction. Legacy startup validates trusted
records into explicitly labeled current-state baselines, resumes partial migration
without duplicate rows, and rejects version drift. Ready history stays readable
when acquisition is disabled. It grants no execution or acceptance authority.
See [ACQUISITION_HISTORY.md](ACQUISITION_HISTORY.md) for endpoints and limits.

All **560 Rust workspace tests** pass with `serde_json/preserve_order` (62 suites,
zero failed/ignored), formatting and strict all-target Clippy pass. Nine module and
nine HTTP/storage tests exercise ACLs, redaction, pagination, atomic rollback,
attention/revocation, restart, partial migration, drift and Unicode IDs. Provider
replies remain loopback fixtures; no live sponsor execution is claimed.

Claude keeps planner/general lifecycle/saga/outbox, provider activation, Elastic and
UI; #11/#12 heads remain unchanged. The ongoing Codex goal remains active. The next
capability slice will be claimed in issue #4 before shared edits. Dependency order:
#8 → #9 → #10 → #13 → #14 → #15 → #16 → #17 → #18.

### Previous automatic scenario checkpoint

Codex has pushed [PR #17](https://github.com/HenryWashuHe/genesis/pull/17),
`codex/automatic-scenario`, implementation `f92ac15`, stacked on #16. Optional
`progression: "automatic"` creates authorized acquisition/run intents and observes
server workers through status and immutable proof. Existing specs default to manual;
the example selects automatic. Preflight requires A's opt-in automation before
mutation. Version-skewed acquisition/scheduler reads wait for a consistent view;
attention stops without manual fallback. Collection/finalization preserve the exact
artifact/input/suite/acceptance/receipt proof checks. Polling defaults now fit the
30-minute overall budget. Driver timeout does not cancel authorized server work.

All **542 workspace tests** pass with `serde_json/preserve_order` (61 suites, zero
failed/ignored); formatting and strict all-target Clippy pass. Five integration tests
run actual `serve_org` instances behind HTTP guards rejecting every `/step` and
`/verify` POST. The full scenario completes repair, independent B acceptance, both
role handoffs and fresh successor execution after B restart, with zero forbidden
requests. Wrong fresh output stops before handoffs; an ambiguous Actor POST remains
unreplayed after A restart; disabled automation leaves both organizations empty.
Six pure observation tests and four bounded HTTP tests cover races, attention,
manual compatibility, limits and invalid proof responses. Independent review found
no material blockers. Evidence remains canned provider HTTP replies and DevStub roles.

Next Codex work: acquisition listing and redacted immutable history, so operators
can find work after losing an ID and inspect progress after restart. Keep journal
writes atomic with existing acquisition/checkpoint/quota changes, expose only bounded
operator metadata, and label legacy baselines honestly. Exact scope/claim belongs in
issue #4 before code edits. Claude retains planner/general lifecycle/saga/outbox,
provider activation, Elastic and UI; #11/#12 heads remain unchanged. The ongoing goal
is active. Dependency stack: #8 → #9 → #10 → #13 → #14 → #15 → #16 → #17.

### Previous automatic acquisition checkpoint

Codex has pushed [PR #16](https://github.com/HenryWashuHe/genesis/pull/16),
`codex/automatic-acquisition`, implementation `b71d7b9`, stacked on #15. Explicit
`--auto-acquire` drives committed capability intents through generation, build,
verification and one allowed semantic repair. Bounded fair scheduling persists
checkpoint versions, pacing and redacted attention. Existing authority, quota,
unknown-POST and explicit-read-retry gates remain authoritative. Paused quota/read
attention is reconciled locally for revocation without provider calls. Automatic
mode requires policy and both clients; it defaults off. Publication/install remain
separate. Operator-only `GET /capability-acquisitions/{id}/automation` reports
scheduling metadata, never a semantic proof. See [SERVER_ACQUISITION.md](SERVER_ACQUISITION.md).

All **531 Rust workspace tests** pass with `serde_json/preserve_order` (61 suites,
zero failed/ignored), formatting and strict all-target Clippy pass. The 18 acquisition
HTTP tests include actual `serve_org` success/repair without manual steps, durable
attention, restart/cancellation and authority races. Eight scheduler tests cover
bounded fairness, timing, version checks, rollback and status/configuration safety.
These remain loopback provider fixtures, not paid/live sponsor evidence.

The next Codex milestone connects the existing two-organization scenario to these
automatic workers, so it observes acquisition and invocation proofs instead of
advancing each step. Scope: scenario driver/tests/docs, narrow health fields and CLI
scenario help/tests if needed. The overarching goal remains active. Claude retains
planner/lifecycle/saga/outbox, provider activation, Elastic and UI. PR #11/#12 heads
remain unchanged; their requested review fixes remain assigned to Claude.
Dependency stack: #8 → #9 → #10 → #13 → #14 → #15 → #16.

### Previous automatic invocation checkpoint

Codex has pushed [PR #15](https://github.com/HenryWashuHe/genesis/pull/15),
`codex/automatic-invocation`, implementation `88413fd`, stacked on #14. The server
now automatically finalizes collected bound invocations through the same proof and
authority gate. A separate local worker processes at most 16 eligible jobs per pass,
persists claims and retry timing, and limits interrupted/revision-conflict recovery
to three attempts. Deterministic failures enter durable attention without starving
healthy jobs. No provider calls, external-effect retries or ledger writes occur in
this worker. Operator-only `GET /capability-runs/{id}/finalization` reports safe
scheduling metadata; the existing proof endpoint remains the semantic authority.
Explicit verification can recover an attention job after its underlying issue is
fixed, atomically completing its attempt row with proof/status/events. Server drop
cancels the worker. See [CAPABILITY_EXECUTION.md](CAPABILITY_EXECUTION.md).

All **509 Rust workspace tests** pass with `serde_json/preserve_order` (61 suites,
zero failed/ignored); formatting and strict all-target Clippy pass. This includes
8 queue/recovery unit tests and 19 invocation HTTP tests. Real-server fixtures prove
automatic supported/unsupported/mismatch verification, collection before restart,
manual/worker convergence, malformed-record isolation, durable attention, bounded
retries/batching, transactional rollback and cancellation. Independent review found
no additional material issues under the documented single-process database model.

The preceding [PR #14](https://github.com/HenryWashuHe/genesis/pull/14),
`codex/capability-invocation`, freezes installation/request/full-suite/acceptance
bindings and introduces a distinct single-case verifier. Historical proofs preserve
the saved authority revision/time after retirement and restart; fresh successor
work still needs current authority. Complete-record digests cover receipts even
when authority rejection produces no semantic report. Matching trusted catalog
configuration remains required. Legacy runs are not retroactively bound.

The full two-organization scenario requires supported fresh proofs, including after
handoff and SQLite restart. Acceptance jobs remain `VERIFYING`; verified invocations
become `SUCCEEDED`. Provider/model responses are loopback protocol fixtures and
created roles are labeled DevStub; no live sponsor or paid model calls were made.
Both ledgers remain empty. Truthful unsupported extraction is not an available venue.

The overarching goal remains active. Next Codex investigation is opt-in automatic
capability acquisition: progress an already authorized requirement through the
existing durable generation/build/verification/repair runtime without manual step
polling, preserving count quotas, fresh authority and unknown-POST/read-retry gates.
Inspect remote changes and claim exact paths in issue #4 before implementation.
Planner/general saga remains outside this scope.

Claude has now opened [PR #11](https://github.com/HenryWashuHe/genesis/pull/11)
(`feat/elastic`, `4d4949c`) and
[PR #12](https://github.com/HenryWashuHe/genesis/pull/12)
(`feat/planner`, `5f03bb5`), both based on main `5c4dd1a`. Codex requested changes
on both after isolated executable reproductions. Elastic needs collision-resistant,
consistent tenant naming, exact binding/tool readiness and ambiguous-create GET
reconciliation; five regression tests are pushed as `codex/review-elastic-pr11`,
commit `73ccca8`. Planner needs durable observation operation identities, disabled
redirects and sanitized provider errors; five failing regression tests and one
passing control are pushed as `codex/review-planner-pr12`, commit `7c869b0`. These
are test-only review branches, intentionally failing on the reviewed heads, not
production fixes or additional merge-ready PRs. Reviews:
[Elastic](https://github.com/HenryWashuHe/genesis/pull/11#pullrequestreview-5608329909),
[planner](https://github.com/HenryWashuHe/genesis/pull/12#pullrequestreview-5608329992).
Claude retains those fixes. The capability dependency stack
is #8 → #9 → #10 → #13 → #14 → #15. Planner/lifecycle/saga, provider activation, Elastic and UI
remain Claude-owned; the whole-kernel credit fallback remains inactive.

## Prior integration checkpoints

Codex has pushed [PR #10](https://github.com/HenryWashuHe/genesis/pull/10),
`codex/server-acquisition`, code checkpoint `aaefa12`, stacked on #9. New server
endpoints create/step/report a SQLite acquisition bound to a committed requirement,
role authority, assignment/epoch, lease and deadline. Provider intent and durable
count quotas commit together before I/O. Stale authority permanently fences work
while preserving provider receipts. Verified acquisitions can publish via
`verification_ref: acquisition:<id>` after fresh authority checks and independent
reverification. See [SERVER_ACQUISITION.md](SERVER_ACQUISITION.md).

All **453 Rust workspace tests** pass with `serde_json/preserve_order`, zero
failed/ignored. Formatting and strict all-target Clippy pass; the seven CLI tests
also pass after the final argument-struct cleanup. The 27 new tests cover store,
runtime, HTTP and CLI, including complete acquisition and actual HTTP publication
to a loopback registry, restart, quota persistence, concurrent steps, revoked
permissions, prompt/status privacy and foreign proofs. No live sponsor or paid
model calls were made. Retained acquisition records require the matching embedded
scaffold; count quotas are organization-local action bounds, not dollar limits.

The overarching goal remains active. Next Codex work should connect this acquired
proof to the two-organization inheritance demonstration and close any reproduced
capability-runtime authority gaps without editing Claude's planner/saga/UI paths.
Check current remote branches and issue #4 before claiming the next slice. Main
remains `5c4dd1a`; the current open dependency stack is #8 → #9 → #10. Claude's
`feat/planner` remains `caac0dc` at this checkpoint; that does not establish credit
exhaustion or authorize a whole-kernel takeover.

Codex has pushed [PR #9](https://github.com/HenryWashuHe/genesis/pull/9),
`codex/capability-acquisition`, code checkpoint `6853f16`, stacked on PR #8. One
`AcquisitionSession` now carries generation, Actor packaging/build and independent
verification, with one preplanned semantic repair. The CLI exports a reverified
backend-compatible ForgeSession proof. The normalized Apify receipt bound closes
JSON expansion beyond checkpoint headroom. See
[CAPABILITY_ACQUISITION.md](CAPABILITY_ACQUISITION.md) for commands and contracts.
All **426 Rust tests** pass with `serde_json/preserve_order`, zero failed/ignored;
workspace formatting and strict all-target Clippy pass. No live provider calls
were made. Use the same embedded scaffold/binary through each acquisition.

The user has requested ongoing autonomous implementation across milestones.
The next slice announced at that checkpoint was mission-bound server acquisition,
now implemented in PR #10 above: durable SQLite
intent/step/status, current assignment/lease/deadline checks, persistent count
quotas and trusted publication-proof lookup. New capability modules/routes and
narrow app registrations/evidence lookup are Codex-owned; Claude keeps planner,
core lifecycle/saga, asynchronous provider activation, Elastic and UI. Allocation
budgets are not a provider-spend ledger; the next slice must not claim dollar
billing enforcement. Ownership and exact integration checkpoints are also recorded
in [issue #4](https://github.com/HenryWashuHe/genesis/issues/4).

PR #3/#5/#6/#7 are merged on main at `5c4dd1a`. Codex has pushed source
synthesis in [PR #8](https://github.com/HenryWashuHe/genesis/pull/8), branch
`codex/capability-synthesis`, code checkpoint `f91d6ef`, in the forge modules/CLI/tests, with
public-example inputs, a durable one-shot model adapter and exact generated-source
handoff. [CAPABILITY_SYNTHESIS.md](CAPABILITY_SYNTHESIS.md) defines the interface
and remaining live gates. Claude retains asynchronous role activation, Elastic,
planner and UI; this does not activate the whole-kernel fallback.
The combined workspace passes 395 Rust tests with `serde_json/preserve_order`,
workspace formatting and strict all-target Clippy. The 29 new tests cover the
model adapter/state and CLI handoff. No paid model or cloud Actor calls were made.

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
