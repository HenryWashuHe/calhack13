# Fetch independent specialists

GENESIS now has a Rust recruitment journal and a separate signed Fetch transport.
The implementation can negotiate a contract with an independently running specialist,
collect correlated task evidence, survive process restarts and release one
organization without terminating the specialist's other contracts.

This milestone does not activate kernel roles. Native contract acceptance, kernel
assignment, transport delivery and verified mission fulfillment remain separate.

## Implementation

- `genesis-fetch::discovery` calls the documented Agentverse search API with an
  exact protocol filter, bounded pagination and authenticated HTTP. Candidates must
  advertise the expected protocol; results are untrusted metadata and never become
  executable peer URLs automatically. Both documented protocol-list shapes are
  normalized. No candidate is automatically recruited or appointed.
- `genesis-fetch::protocol` defines a versioned request/offer/accept/task/release
  vocabulary inside the SDK-signed payload. Organization, mission, contract,
  correlation, task, attempt, assignment, epoch and deadlines travel explicitly.
  Strict decoding rejects ambiguous JSON, unknown wire fields and oversized data.
- `genesis-fetch::runtime` stores immutable outgoing intents, contracts, task
  evidence and a deduplicated inbox in SQLite. Contract mutation plus outgoing
  intent is atomic. Incoming dispositions, evidence, contract changes and the
  bridge polling cursor also commit atomically. Exact retries do not repeat state
  changes; changed message identities conflict. Invalid semantic replies are
  retained as rejected evidence so one reply cannot block the inbox indefinitely.
- `genesis-fetch::client` authenticates the private bridge, checks its expected
  identity before sending payloads, and bounds that preflight and POST by one
  deadline. Queue receipts bind message, recipient and bridge. HTTP acceptance
  acknowledges a durable local queue only. An uncertain queue POST can be retried
  with its exact immutable message because this GENESIS bridge supplies deduplication.
- `services/fetch-bridge` uses pinned Fetch SDK `Identity`, `Envelope` and `Model`
  primitives with bounded HTTP. It verifies signatures, target, schema and expiry
  before saving an incoming message. The authenticated native sender is preserved
  separately from application content. It does not run the SDK's automatic public
  registration/status behavior. See its README for the upstream contracts and
  exact installation lockfile.

Releasing a contract immediately suppresses its unsent task requests in the Rust
journal. A request already queued or in flight may still reach the specialist;
late replies cannot reopen the released Rust contract. Release/ack cleanup messages
may outlive the negotiated lease, but must have fresh envelope expiry and retain
all identity/context checks. This is not remote cancellation or a guarantee of
exactly-once execution.

The optional Python specialist is a synthetic advisory example. It owns its own
persistent per-organization contracts and idempotent task receipts. Its output
explicitly says `synthetic:true` and `domain_verified:false`; it performs no external
business action. Two organizations use separate Rust journals and bridge identities.

## Immutable receipt reconstruction

The Rust journal exposes `recruitment_receipt(peer, expected_recruit,
expected_accept)` and `task_receipt(peer, expected_recruit, expected_accept,
expected_task)`. Supply the complete frozen outgoing messages, including the exact
accepted offer identity and task input, grant, attempt, idempotency key and deadline.
The accessors read one SQLite snapshot and reconstruct the immutable evidence
chain from prepared outbox rows and authenticated inbox rows whose disposition is
`Applied`. A current `Active` or `Collected` label alone is insufficient.

Receipts bind the organization, configured bridge and selected peer, then check
message identities, context, cross-message links and stored digests. Their hashes
cover the relevant request/response chain, excluding later tasks, release, cursor,
queue acknowledgments and mutable contract revision. Historical reconstruction
therefore survives expiry and release; it does not authorize new work under an
expired or revoked grant. A missing or inconsistent chain returns an error without
preparing a request, retrying delivery or changing kernel state.

JSON floats retain their exact binary value through journal decoding. Canonical
request identity distinguishes `0.0` from `-0.0`; receipts hash the authenticated
stored messages rather than an equal-looking caller replacement. Eligible sends
also verify their original outbox digest and cached request. Legacy rows whose
float values already drifted fail closed instead of being rehashed or replayed.
Expired or fenced tasks are skipped before those send checks, so historical drift
cannot block a contract release or unrelated eligible work. No hash-domain or
journal-schema migration is performed.

The returned Rust types have private construction and serialization support,
without deserialization into a trusted receipt. Debug formatting hides task input,
output and offer terms. Serialized receipts contain evidence for trusted backend
consumers; treat them as private data. A serialized object is not an independently
verifiable signed envelope and must not be accepted from an HTTP caller as proof.
The journal retains the authenticated sender and canonical message after the
bridge verifies the SDK signature, rather than the complete native envelope.
It also stores no ingestion timestamp, so these accessors invent none.

Transport provenance remains separate from domain verification. A specialist can
return a well-correlated but wrong answer, and a previously authorized task can
finish after its authority is revoked. The later server integration must validate
the result against verified source facts and recheck current kernel authority
before applying any mission effects. The operator-started
[venue recovery coordinator](VENUE_RECOVERY.md) implements this boundary on its
work-in-progress branch; validation and live evidence remain separate. The earlier
[scoped mission bridge plan](FETCH_MISSION_BRIDGE_PLAN.md) preserves its rationale.

## Run the complete local proof

```sh
cargo build --locked -p genesis-fetch
uv sync --project services/fetch-bridge --frozen --python 3.12
uv run --project services/fetch-bridge --frozen python scripts/demo-fetch-recruitment.py
```

The script starts three separate Python processes, then uses the actual Rust CLI
for both organization journals. It negotiates two contracts with the same independent
specialist, collects one task per organization, restarts the specialist and Org A's
bridge, releases A, and collects another task for B. The report includes native
identities and durable message IDs. All subprocesses and temporary stores are
cleaned up; `--output report.json` retains the report if requested.

This proof uses native signed HTTP, not a same-process SDK dispatcher or a mocked
signature. Its peers and identities are local public test fixtures. It neither
contacts Agentverse nor proves public discovery, registration or remote reachability.

## Backend CLI

Credentials are environment-only. `prepare` and `status` are offline. Message files
are trusted backend inputs; the CLI is a proof tool, not an agent-facing gateway.

```sh
# Requires AGENTVERSE_API_KEY; performs a real discovery request.
cargo run -p genesis-fetch -- discover --query 'logistics specialist' --limit 10

cargo run -p genesis-fetch -- prepare --db data/fetch-a.sqlite --org org_a \
  --bridge-address '<organization agent1 address>' --peer '<specialist agent1 address>' \
  --message recruit.json

# Requires GENESIS_FETCH_BRIDGE_TOKEN; sends eligible prepared messages.
cargo run -p genesis-fetch -- exchange --db data/fetch-a.sqlite --org org_a \
  --bridge-address '<organization agent1 address>' --bridge-url http://127.0.0.1:8101/v1

cargo run -p genesis-fetch -- status --db data/fetch-a.sqlite --org org_a \
  --bridge-address '<organization agent1 address>' --contract contract_a
```

Create the private parent directory first. Keep journal files private. Runtime
metadata binds one organization and bridge identity; changing either refuses to
open the existing journal. The Rust store limits each table to 10,000 records,
each contract to 128 tasks, and normal stored JSON payload to 64 MiB measured in
UTF-8 bytes. A bounded cleanup reserve permits one release per contract: outgoing
rows may reach 20,000 and total payload 256 MiB for release transactions only, so
the normal soft limit cannot prevent local revocation. Normal contract checkpoints
leave 64 KiB below the decoder’s 8 MiB ceiling for release evidence. Actual SQLite
disk/transaction failures still fail explicitly. Indexes/journals require additional
space. Capacity failure rolls back the entire transaction. There is no automatic pruning or identity reset.

Use one active exchange driver per organization. A prepared task is immutable, but
preparation does not prove that its assignment remains authorized. The kernel hook
must check live authority before every dispatch and result application. The CLI
rechecks local contract eligibility between HTTP calls; this is not a substitute
for kernel authorization or a cross-process cancellation barrier.

## Kernel handoff for Claude

1. Persist a recruitment intent and selected candidate in the kernel's own change
   transaction/outbox. A search advertisement is not proof of capability or consent.
2. The configured bridge identity and peer endpoint must be established separately.
   Advance negotiation outside the kernel lock. Keep existing ownership until the
   native contract is accepted and bridge readiness has been independently proven.
3. Bind the native peer/contract to the kernel role and lease. Do not implement this
   asynchronous negotiation as a synchronous successful `RoleProvisioner` receipt.
4. At dispatch and result application, recheck role status, current assignment,
   delegation generation/epoch, scoped grant and deadline. `TaskResult` is collected
   evidence only; domain verification and ledger changes remain kernel-owned.
5. Persist delivery attempts and resume the exact message ID. Retire by releasing
   the local contract, fencing future effects and transferring obligations. Do not
   shut down an independent specialist that may serve another organization.

The PR #3 A→B→A delegation-fencing follow-up is resolved by `7a00da4`: each changed
executor rotates the assignment and epoch, including when the old executor returns.
Existing owner work also becomes stale and must be redispatched. The per-change
process-local saga guard in `1460831` prevents overlapping drivers in one server.
These narrow merge-readiness fixes do not implement the asynchronous role activation
or live-authority checks required for Fetch integration. Claude retains that hook,
the planner and UI.

## Validation and remaining live gate

Rust tests cover HTTP identities/bounds, both discovery response shapes, complete
negotiation, sender/context/epoch/deadline fences, out-of-order results, restart,
idempotency and SQLite rollback boundaries. A Unicode storage regression ensures
the payload limit counts bytes. Python tests cover actual separate-process signed
HTTP, tampering, transport replay/restart and independent contract release.

Before claiming public sponsor integration, register/advertise a real independently
operated specialist, verify an authenticated Agentverse search returns it with the
correct protocol, configure its authenticated identity and reachable signed ingress,
and run negotiation/task/release from the Rust backend. Record native addresses and
receipts. No authenticated search or public registration was performed here.

The official [search tutorial](https://innovationlab.fetch.ai/resources/docs/agentverse/searching)
still specifies `POST https://agentverse.ai/v1/search/agents`; the
[v1 reference](https://docs.agentverse.ai/v-1/api-reference/search/agents) is marked
deprecated. No unverified replacement endpoint is assumed. Reconfirm that API on
the team's account during the live smoke test.
