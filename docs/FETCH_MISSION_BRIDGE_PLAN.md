# Proposed Fetch mission bridge

**Status: the immutable receipt milestone is implemented at `afbe0bf` (original
source `6424bfb`); the server mission bridge below remains future work.** All 64
Fetch crate tests pass, including 13 receipt tests, with strict crate all-target
Clippy, workspace formatting and clean independent review. The receipts use the
private journal's authenticated ingestion boundary; these tests are synthetic
protocol fixtures and do not execute the native SDK or cloud providers.

The original assessment used main `1bb3c9f`. Scope was claimed on
[coordination issue #4](https://github.com/HenryWashuHe/genesis/issues/4#issuecomment-6354668197).
Only the Fetch crate and focused documentation changed. Claude retains the general
saga, planner, readiness, UI, gap finalizer and subsequent adopt-before-acquire
coordinator; a server recovery adapter still requires a separate scope claim.

The proposed mission contribution is a recruited replacement-venue specialist
after a verified CREAO assessment detects insufficient capacity. The independent
specialist selects from backend-verified candidate venue facts; an accepted choice
drives a real follow-up assessment for the outstanding replacement requirement.
It does not make a reservation, establish live availability or satisfy the whole
mission merely by returning a result.

## Implemented first milestone: immutable Fetch receipts

The bounded, serialize-only receipt API is in
[`genesis-fetch::runtime`](../crates/genesis-fetch/src/runtime.rs), with focused
tests in [`genesis-fetch/tests`](../crates/genesis-fetch/tests). This is an
independent prerequisite for later server domain work.

Today `RuntimeStore::prepare` saves immutable outgoing messages and contract changes
atomically. `ingest` saves reply dispositions, contract/task state and the bridge
cursor atomically. However, `get` validates stored message shapes and context; a
`Collected` label alone is not a validated execution receipt. The contract's
`acceptance` field contains the outgoing `AcceptOffer`, not the received
`ContractAccepted` message.

The receipt accessors reconstruct the relevant evidence from the private journal
and check:

- The caller's exact frozen recruitment/task request, organization and bridge
  identity, peer, mission, contract, correlation, job, attempt, assignment, epoch,
  idempotency key and deadline.
- The persisted outgoing request and linked offer, accept request and native
  acceptance. Incoming records must have the expected authenticated sender,
  valid stored digest and an `Applied` disposition.
- The collected `TaskResult` must name the exact task request, job, attempt and
  epoch. A rejected inbox record, unrelated result or bare phase label is
  insufficient.

They return native message/peer identifiers, bounded output and domain-separated hashes
of the immutable request, response and evidence chain. Do not hash the entire
mutable contract as the historical receipt identity: later tasks and release must
not invalidate earlier evidence. Redact output in `Debug`; offer no `Deserialize`
implementation or `domain_verified` flag. Reading a receipt must perform no
preparation, HTTP call, retry or kernel mutation.

This is provenance from the authenticated private bridge and validated journal.
The current bridge stores authenticated sender plus canonical message after SDK
signature verification; it does not retain a self-contained signed envelope for
independent public verification. A hash must not be described as such a signature.
Historical receipt reconstruction should survive expiry/release; applying its
result still requires fresh kernel authority. Do not invent missing legacy
evidence or receipt timestamps during migration.

The 13 receipt tests cover real runtime preparation and ingestion with synthetic
protocol messages: exact request
binding and cross-organization rejection; missing, pending and rejected evidence;
changed sender/body/digest/link rejection; restart and zero-I/O reads; receipt
identity surviving an unrelated later task and contract release; bounded output
and redacted debug formatting. No native SDK signature or provider execution is claimed by these receipt tests.
See [the API and evidence boundary](FETCH_RUNTIME.md#immutable-receipt-reconstruction).

## Later bounded server integration

There is currently no Fetch runtime/client registration or exchange worker in
`genesis-server`. The Python
[`Specialist`](../services/fetch-bridge/genesis_fetch_bridge/specialist.py) returns
a synthetic advisory rather than a replacement-selection computation. Both are
missing implementation, not configuration switches.

A proposed operator-created recovery intent freezes the reopened assessment and
requirement, supervising role/responsibility and grant, committed contractor,
explicit event policy, a bounded set of verified candidate invocation IDs, and
deadline. Persist complete outgoing messages and IDs before crossing into the
separate Fetch journal; recovery must replay the exact preparation rather than
minting another contract or task. Count budgets, terminal attention and scheduling
limits must be explicit.

Use existing `RecruitSpecialist`, `AssignResponsibility` and
`DelegateTask(Contractor)` operations under an accountable existing role. Give the
recovery work its own responsibility: delegation rotates its assignment/epoch and
must not fence the continuing CREAO monitor accidentally. This does not require
pretending that native contract acceptance provisioned a Fetch kernel role.

Negotiate with an operator-configured peer. Optional authenticated discovery may
confirm that peer advertises the required protocol, but advertisement metadata is
not authority and must never supply an automatically trusted routing URL. Task
input contains only verified extraction facts, policy and proof references—not
HTML, oracle answers or expected specialist output. A narrow independent
specialist returns structured candidate selection and exact evidence references.

Backend verification checks source membership, citations and the explicit policy
conditions. An accepted choice can supply the source and selected venue for an
existing CREAO assessment intent, with a proof-linked observation for the recovery
work. Preserve the original replacement requirement as outstanding until its
acceptance predicate is actually established. The current core has no general
requirement-satisfaction helper; this plan does not authorize an ad hoc status
write or broader lifecycle redesign.

## Required integration boundaries

- **Managed contracts and jobs:** the existing contract-state endpoint accepts a
  caller-supplied state, and generic job results accept adapter assertions. Newly
  managed Fetch records must reject these alternate write paths. Only the
  validated native receipt plus backend domain decision may finalize them.
- **Current authority:** check supervising role, scope, responsibility, assignment,
  epoch, mission, lease, job deadline and the exact live contractor binding before
  dispatch and again before applying output. Use `send_guarded` after the awaited
  bridge identity preflight. Preserve late received evidence without granting it
  mission effects. Already queued transport is not remotely cancelled by a local
  release; exact-ID redelivery relies on receiving deduplication.
- **Source fixtures:** the current capacity-cut fixture has no eligible
  replacement; its cards case describes another event. A positive scenario needs
  a same-event replacement snapshot, B oracle case and source allowlist frozen
  before startup/acquisition, then a genuinely verified invocation. Do not reuse
  an unrelated event or add expected answers after seeing specialist output.
- **Identifiers and bounds:** Fetch wire IDs are ASCII and at most 128 bytes;
  server domain IDs permit more. Explicitly reject unsupported bindings before
  effects, or introduce a persisted exact alias mapping. Never silently normalize
  identities. Protocol messages and nested input/output have existing byte limits.

## File seams and evidence requirements

| Existing seam | Proposed use |
| --- | --- |
| [`genesis-fetch/runtime.rs`](../crates/genesis-fetch/src/runtime.rs) | Immutable receipt API and exact journal replay |
| [`genesis-fetch/client.rs`](../crates/genesis-fetch/src/client.rs) and [`discovery.rs`](../crates/genesis-fetch/src/discovery.rs) | Bounded private bridge exchange and optional candidate discovery |
| [`fetch-bridge`](../services/fetch-bridge/README.md) | Separate signed transport and a narrow domain specialist implementation |
| [`capability_authority.rs`](../crates/genesis-server/src/capability_authority.rs) | Current supervising grant, mission and lease checks |
| [`capability_invocation.rs`](../crates/genesis-server/src/capability_invocation.rs) | Resolve independently verified source facts from backend evidence |
| [`venue_assessments.rs`](../crates/genesis-server/src/venue_assessments.rs) | Existing assessment intent/proof and atomic mission-effect pattern |
| [`capability_scenario.rs`](../crates/genesis-server/src/capability_scenario.rs) | Optional child workflow after the accepted `reopen` result |

Possible new ownership is limited to a server `venue_recovery` module, pure
selection verifier, narrow exchange adapter, routes and focused tests. Shared
registration, managed-result guards and scenario changes need explicit coordination;
do not replace the general outbox/specialist/saga frameworks.

Before claiming mission integration, exercise separate signed bridge/specialist
processes through negotiation, selection and real backend verification, then a
CREAO follow-up driven by the selected source. Test restart at each cross-journal
boundary, exact replay without another task, authority revocation while waiting,
foreign/stale results, malformed or semantically wrong selections, failure to
persist final effects, and isolation between independent organizations/contracts.
Preserve native IDs and immutable proof references without exposing private output.

The existing [Fetch runtime guide](FETCH_RUNTIME.md) and
[two-organization transport demo](../scripts/demo-fetch-recruitment.py) establish
the local transport baseline. This plan adds no claim of public Agentverse
registration, authenticated cloud discovery, native provider execution or completed
all-sponsor integration.
