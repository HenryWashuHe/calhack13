# Fetch venue recovery

This opt-in implementation has passed the local integration checks below.
Cloud-provider validation and the complete autonomous mission remain open.

After an accepted CREAO assessment reopens an event requirement, an operator can
start bounded recovery using already verified candidate venue invocations.
GENESIS gives that recovery its own responsibility, recruits an independently
running Fetch specialist, verifies its selection, and sends the selected source
to the existing CREAO monitor for a fresh assessment. The original monitor's
responsibility and assignment remain unchanged.

This is an operator-started workflow. It does not automatically search for venues,
invoke their Actors, create a new CREAO agent, or start recovery from every Reopen.
Those inputs and the monitor must already exist. The earlier
[mission bridge plan](FETCH_MISSION_BRIDGE_PLAN.md) records the design rationale;
this guide describes the narrower API and current implementation.

## Configure the participants

Start an organization bridge and a separate specialist using the
[Fetch bridge setup](../services/fetch-bridge/README.md). Use the specialist's
explicit `--specialist-mode venue_selection`; the default advisory mode does not
implement the required `venue.select` contract. Each process keeps its own seed,
private token, SQLite database and configured peer routing. Keep the bridge's
private `/v1` API separate from signed peer ingress.

The organization server reads all five variables together when `serve_org`
starts. Supplying only some of them, or an empty value, fails startup.

| Variable | Meaning |
| --- | --- |
| `GENESIS_FETCH_BRIDGE_URL` | Trusted private organization bridge API base, such as `http://127.0.0.1:8101/v1`. |
| `GENESIS_FETCH_BRIDGE_TOKEN` | Secret Bearer token for that bridge. |
| `GENESIS_FETCH_BRIDGE_ADDRESS` | Expected public native identity of the organization bridge. |
| `GENESIS_FETCH_SPECIALIST_ADDRESS` | Operator-selected native peer identity; it must differ from the organization bridge. |
| `GENESIS_FETCH_RUNTIME_DB` | Private Rust Fetch journal, separate from the organization and Python bridge databases. |

With all five absent, new recovery is unavailable. Existing local retirement can
still run, but remote cleanup retains its exact intent with `needs_attention`.
Preserve all journals and stable identities across restarts. One organization
process and one exchange driver per journal are supported; the process mutex is
not a cross-process lease. Do not run the Fetch CLI exchange loop against a
journal already driven by this coordinator.

The runtime does not perform Agentverse discovery. An optional separate discovery
check may establish that the configured peer advertises the protocol; search
metadata does not choose a trusted routing URL or authorize recruitment.

Configure the existing [CREAO runtime](CREAO_RUNTIME.md) and retain the active,
CREAO-bound role that produced the Reopen assessment. That role needs both
`contract.logistics` and `monitor.venue`, valid role/mission authority, and the
kernel's capacity for another contract and responsibility. Recovery reuses that
role and agent binding; a Fetch contract is not a newly provisioned kernel role.

## Prepare verified candidate sources

The starting assessment must have an accepted `reopen` proof and its associated
replacement requirement and observation. The original invocation and each
candidate must be a successful, semantically verified Apify invocation in this
organization. The backend reconstructs their private invocation and assessment
proofs; caller-supplied venue JSON, acceptance-test runs and copied proof objects
cannot replace those records.

Every candidate must share the original invocation's mission, installation,
complete suite identity/hash, acceptance reference and artifact pin. Invocation
inputs must be identical after removing only `source_url`; canonical JSON
identity preserves the distinction between positive and negative zero while
ignoring object key order. Each input's removed URL must equal its verified source
snapshot URL. Every venue citation must match that source's URL and snapshot hash.
This permits another verified page for the same frozen event, not a different
event or capability that happens to return plausible venues.

Supply 1–8 distinct source job IDs, containing 1–16 venues in total. Candidate
venue IDs must be globally distinct, and source citations must already use the
canonical URL spelling accepted by the Rust/Python contract. Unsupported or empty
extractions are refused. The failed venue may be present in the supplied evidence
but is always excluded from selection.

The work deadline must be in the next 30 minutes and no later than 30 minutes
after **both creation and verification of every source**, including the original
one. Source timestamps cannot be in the future. This bounds the age of recorded
execution evidence; it does not establish when the website itself was fetched,
current availability, or facts beyond the frozen snapshots. Historical
reconstruction never renews freshness or the work deadline.

## Start and inspect recovery

Use an operator token for `POST /api/v1/venue-recoveries`:

```json
{
  "client_request_id": "event-recovery-1",
  "reopened_assessment_id": "<accepted-reopen-assessment>",
  "candidate_job_ids": ["<verified-same-event-invocation>"],
  "deadline": "<RFC3339 time within every source freshness window>"
}
```

The request key is 1–128 ASCII letters, digits, underscores, periods, colons or
hyphens. The deadline may have at most microsecond precision. Save the exact
request, including candidate order and deadline. First creation returns `201`;
repeating the same key and request returns the existing record with `200`.
Changing any field under that key conflicts. There is one persisted recovery per
replacement requirement, so a different key cannot start a second recovery for
the same requirement.

Creation atomically commits a new responsibility owned by the existing monitor,
a requested Fetch contract, delegation of that responsibility to the contractor,
a managed job, and complete frozen recruitment/task intents. It does not send
provider HTTP while holding the kernel lock. The native offer must advertise
exactly `venue.select` with the requested lease; immutable recruitment evidence
must establish acceptance before the kernel contract becomes accepted.

| Route | Authorization and behavior |
| --- | --- |
| `GET /api/v1/venue-recoveries/{id}` | Operator or agent; reads the saved public projection. |
| `POST /api/v1/venue-recoveries/{id}/step` | Operator; advances one bounded step, which may perform bridge HTTP. |
| `POST /api/v1/venue-recoveries/{id}/retry` | Operator; clears an operational hold under the rules below, retaining existing identities. |

The normal server worker runs every second and considers at most 16 due records
per pass. An unreadable or failing record is deferred for 60 seconds through its
scheduling index, without rewriting its sealed evidence or authority, so it does
not continually occupy the front of the queue.

The response includes `recovery_id`, `phase`, `attention`, requirement,
responsibility, contract and job IDs, exact recruit/accept/task message IDs,
`receipt_sha256`, `proof_ref`, `selected_venue_id`, `assessment_id`,
`assessment_proof_ref`, and optional `retirement`. It omits the raw task facts and
message output. A saved public projection is not a new independently verified
native receipt.

## Selection and the CREAO child

Phases progress through `recruiting`, `accepting`, `selecting` and `assessing`.
The Fetch task contains frozen facts, policy and hashes using
`venue_replacement_v1`; it contains no expected result, precomputed ranking or
oracle answer. See the [shared contract fixture](../contracts/fixtures/fetch_venue_replacement_v1.json).

The backend reconstructs the task receipt from exact outgoing messages and
Applied authenticated incoming records, then independently checks every result:

- Capacity meets attendance; price uses the exact currency and fits the budget.
- The venue window contains the event window, compared as RFC3339 instants.
- Availability is declared available, and required step-free access is present.
- Evaluations cover every candidate in input order with exact proof and citation
  references. The original failed venue is ineligible.
- Selection is the cheapest eligible venue, with ties ordered by venue ID then
  source job ID in UTF-8 byte order. `selected: null` is valid only when none of
  the supplied candidates qualifies.

Input and output are each bounded to 32 KiB. Strict decoding rejects unknown or
missing fields and ambiguous JSON. A well-correlated native result with incorrect
semantics retains its receipt digest for diagnosis but cannot succeed the job or
create a child assessment.

A verified selection atomically succeeds the managed Fetch job, records a cited
observation and freezes one exact child assessment request. A valid no-match
result also verifies the selection job, but pauses recovery without a child; it
means only that the supplied snapshots contain no eligible replacement. Retrying
that same no-match evidence is refused.

For a selected venue, the child uses the selected source job and original policy
with only `selected_venue_id` changed. It retains the recovery deadline and uses
the same CREAO monitor on the new recovery responsibility. The existing
[assessment verifier](VENUE_ASSESSMENTS.md) owns native execution, unknown outcomes
and semantic acceptance. Recovery becomes `completed` only after that exact
child has an accepted `feasible` proof. A rejected, failed or unknown child
retains its identity and needs attention; it is not replaced with another run.

`completed` means the replacement was independently assessed as feasible. It
does not book a venue, close the replacement requirement, terminate the recovery
responsibility, or complete the mission. The existing kernel has no requirement
satisfaction transition in this workflow.

## Authority and restart behavior

Before work and mission effects, the coordinator rechecks the frozen supervising
role, both scopes, assignment and epoch, requirement, mission, deadline, CREAO
binding, contractor delegation and exact contract state. It reconstructs the
source proofs again. Immediately before a bridge POST, a guarded callback repeats
local authorization after the awaited bridge identity preflight. Authority can
still change during network I/O; late transport evidence cannot by itself grant
mission effects.

The organization transaction saves intents before preparation in the separate
Fetch journal. Restart repeats exact IDs and messages; it does not mint another
contract, task or child to escape an uncertain outcome. Bridge queue submission
relies on the GENESIS bridge's exact-message deduplication. Delivery remains at
least once, and local fencing is not remote cancellation.

A child committed before its link was saved is recovered by its frozen request
identity before fresh dispatch authorization. Once the child exists, recovery can
observe its result without polling the Fetch bridge. After a verified Fetch
selection, preparing the child also avoids a bridge network dependency, though it
still requires the same configured runtime identities and current authority.
Missing or inconsistent source/provider journals require repair; do not delete
evidence or create replacement identities to get past a hold.

Before retirement, automatic sending is limited to three attempts per message.
An explicit `/retry` can reset that automatic batch only after current authority
is revalidated; the original messages, contract, task and deadline remain fixed.
A pre-POST local eligibility refusal consumes no attempt. `/retry` cannot turn
expired work into a new grant or override a no-match decision. Managed Fetch jobs
reject generic result submission, and managed contracts reject generic state
changes; those APIs cannot manufacture consent or verified completion.

## Local retirement and release delivery

Completion or expiry retires the local contract in the same organization
transaction that freezes a cleanup-only `ReleaseContract` message. Requested or
offered contracts become declined; accepted contracts become released. An
unfinished recovery becomes `retired`, and a still-running selection job is
failed locally. Already completed recovery keeps phase `completed`. Existing
child identities and proofs are preserved; retirement is not a claim that a
provider operation was cancelled.

Cleanup has a fresh, fixed deadline 30 minutes after retirement. It does not
extend the work deadline or authorize another task. The public `retirement`
object exposes its reason, time, local contract state, release message ID and
expiry, delivery state, send attempts and attention:

| Delivery | Meaning |
| --- | --- |
| `pending` | Exact cleanup intent remains eligible for bounded processing. |
| `bridge_queued` | The authenticated private bridge acknowledged that exact message into its durable queue. No peer release is proven. |
| `needs_attention` | Missing configuration/journal, inconsistent evidence, uncertain delivery or an exhausted bound requires reconciliation. Local retirement remains recorded. |

Release has **three lifetime send attempts**. Explicit retry cannot reset that
count, replace the release ID or renew its expiry; only a refusal known to occur
before POST refunds an attempt. An exact saved queue acknowledgment can be
recovered after a cross-journal interruption, even after expiry or the final
attempt. Cleanup never fabricates a recruitment to repair a missing Fetch journal.

Preparing release in the Fetch journal suppresses unsent work under that contract.
A message already queued or in flight may still reach the specialist. The
coordinator stops at private bridge queue acknowledgment; it does not wait for or
report authenticated peer release. Local contract retirement frees the kernel's
active-contract capacity independently of that delivery evidence.

## Errors, bounds and evidence limits

Malformed request keys or deadline precision return `400`. Source proof,
freshness, event-binding and candidate failures return `409` with
`venue_recovery_source`; operational conflicts commonly use `venue_recovery`.
Ordinary authentication, kernel validation, missing-record and storage errors
remain possible. A step may instead return `200` with `attention` because it
successfully persisted a hold. Inspect phase and attention, not just HTTP status.

The organization stores at most 256 recoveries, each bounded to 256 KiB, with no
automatic pruning. The separate Fetch and bridge journal limits also apply; see
[Fetch runtime storage and transport](FETCH_RUNTIME.md). Paused records remain
scheduled for deadline retirement. Retirement delivery in `needs_attention`
requires explicit intervention; restarting alone does not renew its budget.

Source-level implementation and fixture tests are not native sponsor evidence.
A complete live demonstration still needs independently operated signed Fetch
participants, actual verified Apify source runs, real CREAO core work and a
genuine CREAO Share link. Loopback authenticated-boundary fixtures do not execute
the native SDK; separate local signed-process tests do not prove Agentverse cloud
discovery. Frozen same-event candidate pages do not demonstrate discovery of an
unseen venue layout. Preserve these distinctions when recording results.

## Validation evidence

The integrated implementation passed 42 venue-assessment tests (including eight
recovery cases), 135 server-library tests, 78 Fetch tests, scoped strict all-target
Clippy and formatting. The Python bridge passed 34 tests, including real
separate-process SDK-signature exchanges and the shared Rust/Python selection
fixture. The default coordinator tests use a synthetic authenticated bridge
boundary and loopback Apify/CREAO responses. The opt-in signed-process check below
also composes the Rust coordinator with two actual Python Fetch processes;
Apify, CREAO and initial provisioning remain the same explicit fixtures.

Recovery cases cover exact receipt and child identity across restart, incorrect
semantics without mission effects, foreign-event/ACL refusal, CREAO progress
during a Fetch inbox outage, corrupt-row scheduling fairness, actual contract-slot
reuse, expiry without a configured runtime, and bounded exact cleanup retries.
These checks establish the tested local behaviors. They do not establish cloud
registration/discovery, live web facts, a genuine CREAO Share link, or booking.

### Composed signed Fetch process check

Install the frozen Python dependencies and explicitly run the ignored integration:

```sh
uv sync --project services/fetch-bridge --frozen --python 3.12
GENESIS_FETCH_TEST_PYTHON="$PWD/services/fetch-bridge/.venv/bin/python" \
  cargo test -p genesis-server --test venue_assessments \
  recovery::native::signed_specialist_selection_completes_real_recovery_and_retires \
  -- --ignored --exact
```

This check passed locally. It starts a native organization bridge and independent
venue-selection specialist with temporary identities, private tokens and journals.
It compares the exact outgoing and authenticated incoming messages with the Rust
receipt, verifies the specialist's actual selection, and restarts both processes
and the Rust organization at the persisted selection boundary. One existing
CREAO fixture agent performs the follow-up assessment; the native task and child
identities remain unchanged.

The test stops the specialist before retirement: the local contract is released
and the bridge queues cleanup while the peer contract is still active. After
restart, it observes the separate authenticated peer release reply. The public
coordinator continues to report only its documented queue acknowledgment.

The default Cargo suite ignores this external-runtime check. An explicit run
fails if its interpreter/setup is missing, rather than silently passing. It has
a 90-second total bound and kills/reaps its owned processes on failure. No cloud
keys are needed. Native envelopes are verified at ingestion; the bridge stores
canonical messages and authenticated senders, not original signed envelopes for
independent public signature verification. This test establishes neither cloud
registration nor a live Apify/CREAO outcome.
