# GENESIS implementation handoff

The October 22 [CREAO sponsor note](CREAO_SPONSOR_NOTES.md) records the required
core agent work, agent ID/run share link, API-family distinction and missing
mission-execution evidence. Keep this separate from readiness and Apify receipts.

Current priority: [review queue and merge dependencies](PR_REVIEW_QUEUE.md).
The user requested a backlog review; capability stack review found no new blockers,
while #11/#12/#20/#21/#22/#26/#27/#28 retain source findings. No PR was merged in this pass.

Latest assembled checkpoint: [draft PR #25](https://github.com/HenryWashuHe/genesis/pull/25),
`codex/combined-integration`, source merge `1004934`.
It combines capability #24, Claude #11/#12/#20/#21/#22/#23/#26/#27/#28 and Codex
provider readiness #29 at their recorded heads. The assembled source passes
699 Rust tests (87 suites), strict Clippy and formatting. The unchanged web/schema
files retain the earlier `dde0682` evidence: 33 web tests, production build and
schema/type regeneration without drift. Rust coverage includes independent
adoption through Elastic, gateway receipts, operator attachment and durable CREAO
probe recovery. No live sponsor evidence is claimed.
See [the full integration matrix](COMBINED_INTEGRATION.md) and [the review queue](PR_REVIEW_QUEUE.md).

This branch remains draft: thirteen reproduced failing regressions across
#11/#12/#20/#26/#27 remain on separate test-only branches, alongside source/UI
findings on #21/#22/#28. They are not part of the passing baseline. Keep source
fixes with Claude and absorb tested new commits here. The current priority is
clearing the review queue. No main merge or whole-kernel ownership transfer is
implied; the ongoing objective and hourly continuation remain active.

### CREAO provider readiness addition

[Draft #29](https://github.com/HenryWashuHe/genesis/pull/29), source `c1e6ba4`,
adds the challenge-bound durable CREAO run, submission-only runtime path,
purpose isolation, redacted Debug output, and `GENESIS_PUBLIC_URL` registration.
The focused branch passed 407 Rust tests, formatting and strict Clippy. See
[CREAO_READINESS.md](CREAO_READINESS.md) for configuration and evidence limits.
The combined checks above include this addition. No live account or installed
skill was exercised. Existing source review findings and ownership remain
unchanged.

### Previous capability checkpoint

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

## Claude checkpoint (issue #4)

Recorded October 22, 2026. Each branch is based on main `5c4dd1a`; together they
merge cleanly in the order below (verified: 437 Rust tests, clippy `-D warnings`,
26 web tests, regenerated schemas/types identical). All provider integration is
tested against in-process fakes only; no live sponsor or model calls were made.

| Order | Branch | PR | Contents | Docs |
| --- | --- | --- | --- | --- |
| 1 | `feat/elastic` | #11 | `genesis-elastic` crate (org-namespaced evidence projection, capability index, Agent Builder `RoleProvisioner` with exact-config reconciliation, fakes, live smoke binary); server projection worker, `/search/evidence`, `/projection`; Elastic specialist jobs as converse turns; registry search through Elastic; console search box; `genesis-elastic-smoke` live smoke binary. PR #11 review fixed with Codex's regressions. | [ELASTIC.md](ELASTIC.md), [ELASTIC_SETUP.md](ELASTIC_SETUP.md) |
| 2 | `feat/planner` | #12 | `genesis-planner` crate; `planner_decision` event; `GET /org-changes/requests/{id}`; idempotent terminal decisions. PR #12 review fixed with Codex's regressions. | [PLANNER.md](PLANNER.md) |
| 3 | `feat/creao-readiness` | to open | Pending/readiness hook (`specialist.rs`), `OrgChangeRecord.operations`, `provisioning_progress` event, specialist jobs (CREAO steps 5–6), operator abandon endpoint, console operation state. | [ASYNC_PROVISIONING.md](ASYNC_PROVISIONING.md) |
| 4 | `feat/ui-evidence` | to open | Console capability evidence: pinned build, publisher verification, local acceptance, per-run backend records. | — |
| 5 | `feat/elastic-planner` | to open, after 1–2 | Planner model as an Elastic Agent Builder mission-planner agent (`GENESIS_LLM_PROVIDER=elastic`). | [PLANNER.md](PLANNER.md) |

Shared-file notes for other branches: `AppState` gains `projection`,
`elastic_agents` and `readiness`; the outbox has arms for specialist jobs and
Elastic jobs before the dev-stub arms; Elastic index and tool names come from
`genesis_elastic::org_namespace` (readable prefix plus a digest of the exact
org ID). None of these touch capability routes or `capability_runtime`.

Open items: live Elastic smoke test (`genesis-elastic-smoke`), CREAO
`CreaoSpecialist` registration in `main.rs` and a CREAO readiness verifier
(bridge receipt), planner model choice, and console display of PR #14
verification proofs once that stack merges.

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
