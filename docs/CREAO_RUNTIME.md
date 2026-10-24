# Persistent CREAO specialists

The Rust CREAO core can create an operational agent, retain its asynchronous
creation identity across restarts, and invoke the same agent for distinct events.
It exposes durable progress through `genesis-providers::SpecialistRuntime` and a
local backend CLI. It does not activate a kernel role or approve a task result.
The asynchronous saga hook and optional provider readiness bridge are implemented;
see [ASYNC_PROVISIONING.md](ASYNC_PROVISIONING.md) and
[CREAO_READINESS.md](CREAO_READINESS.md). The historical hook checklist below
describes the integration boundary; live provider evidence is still required.

## Implemented behavior

- `CreaoClient` uses the Account API at `https://developer.creao.ai/v1`. Production
  cannot override the endpoint; tests have an explicit literal-loopback constructor.
  Authorization, input size, response size, JSON ambiguity, redirects and timeouts
  are checked. Errors contain structured classifications, not provider text or keys.
- Hosted Developer Platform `/v2` is an explicit opt-in via
  `CREAO_API_VERSION=v2` with a project key supplied as `CREAO_ACCOUNT_API_KEY`.
  The default remains `/v1`. The API version is frozen into each operation
  intent and run ticket, so a saved `/v1` operation cannot be submitted or
  polled through `/v2` after a configuration change (or vice versa). Use a
  separate organization journal, account namespace and quota database when
  moving between v1 and v2. Saved operations from another protocol cannot be
  advanced under the new configuration; interrupted records also stop startup
  recovery until the original protocol is restored.
  `/v2` run requests omit the `/v1`-only `should_notify` and `workspace_id`
  fields. Hosted creation polls `GET /v2/agents/{agent_id}/edits/{run_id}`;
  ordinary runs still poll `GET /v2/runs/{run_id}`. If a creation POST returns
  no agent ID, the client cannot construct the documented edit URL and keeps
  the attempted write in `needs_reconciliation` instead of submitting again.
  These endpoints and response shapes follow the [official v2 API reference](https://developer.creao.ai/docs/v2/agents/create).
- `submit` makes one POST. `poll` makes one GET. Tickets bind account, operation
  kind and exact native request. Identities learned later during polling are pinned
  monotonically and saved with the observation, so a restart cannot switch agents,
  conversations, workspaces or models.
- A SQLite journal commits intent before provider I/O. A lost POST receipt remains
  `needs_reconciliation`, with no automatic resubmission. A known ticket resumes
  using GETs; failed reads require explicit retry. Pending runs have persisted due
  times. Checkpoints are size bounded, strictly decoded, context-bound and updated
  with a revision compare-and-swap.
- The shared account database atomically reserves both the request quota and
  management-write quota. Reservations and Retry-After cooldowns survive process
  restart. Two organizations using different keys for one account must use the
  same explicit namespace and quota database. API-key hashes are not account IDs.
- `CreaoSpecialist` derives immutable operations from the full role/job context.
  Separate events reuse a supplied agent binding and get fresh conversations;
  creation conversations are not silently reused for execution. Created progress
  exposes `readiness_verified:false`; collected progress exposes
  `domain_verified:false`. `ProviderSet::specialist` has no development fallback.

The official defaults are 30 account requests/minute and 30 management writes/hour.
An account may have different server-side limits; this implementation's default
limiter is conservative and does not infer overrides. A received 429 extends the
shared cooldown. All attempted POST failures are conservatively treated as possibly
accepted; even documented pre-acceptance errors require operator reconciliation in
this first implementation. [CREAO rate limits](https://docs.creao.ai/developer-api/rate-limits)

## Backend CLI

Prepare and inspect without credentials or network calls:

```sh
cargo run -p genesis-creao -- prepare --db data/creao-org-a.sqlite --org org_a \
  --spec crates/genesis-creao/examples/create-monitor.json
cargo run -p genesis-creao -- status --db data/creao-org-a.sqlite --org org_a \
  --operation demo-create-monitor-1
```

Create the parent data directory beforehand. `prepare` is idempotent only for the
same complete operation. Changing its request, role, organization, account or
assignment under the same operation ID is rejected. Status output omits private
input, raw responses and extracted output.

Set `CREAO_ACCOUNT_API_KEY` in the backend environment. Advancing a prepared
operation sends a real, potentially billable request:

For a Developer Platform project key, also set `CREAO_API_VERSION=v2` before
preparing the operation. A CLI spec must include `"api_version":"v2"`; the
live-gate script sets this field when the same environment variable is set.
Do not use a v2 project key with the default v1 protocol. Existing v1 specs,
tickets and journals keep their original wire identity. The v2 path is covered
by local protocol tests; it has not yet completed a live hosted-agent run.

```sh
cargo run -p genesis-creao -- step --db data/creao-org-a.sqlite --org org_a \
  --operation demo-create-monitor-1 --account shared-demo-creao-account \
  --quota-db data/creao-account-budget.sqlite
```

Call `step` again after `next_poll_at` to observe the existing run. It performs at
most one native operation per call. A retryable failed GET requires `--retry-read`;
that flag cannot replay a POST. The CLI has no URL, token, blind-retry or force
success flag. Keep both SQLite files and their journal files in a private backend
directory. Operation files contain trusted input and are not authentication proofs.

After an interrupted process, run startup recovery before accepting new commands:

```sh
cargo run -p genesis-creao -- recover --db data/creao-org-a.sqlite --org org_a \
  --account shared-demo-creao-account --quota-db data/creao-account-budget.sqlite
```

Recovery makes no provider requests. It preserves known run tickets, marks lost
POST receipts ambiguous, and never manufactures a new agent. The configured
credential is still required to construct the runtime. Use one active runtime per
organization journal. Separate organizations share only the account quota database,
not their operation journals. There is no operator reconciliation/attach-ticket API
yet; preserve ambiguous checkpoints and investigate native provider records.

## Kernel integration hook for Claude

The adapter is deliberately separate from `RoleProvisioner::provision() ->
AgentBinding`. Register it with `ProviderSet::with_specialist`; inspect support via
`configured_specialists()`. It does not advertise itself as a ready synchronous
provisioner or make the existing saga use CREAO automatically.

The next shared change should:

1. Prepare/find a role operation from its stable organization/change/role identity.
   Keep existing responsibility ownership while it is pending.
2. Advance at most one due operation outside the kernel lock, then persist native
   progress. Pending or ambiguous creation must remain visible and recoverable,
   rather than being mapped to a generic terminal provisioning failure.
3. On `Created`, inspect the actual agent via `get_agent`, validate expected
   identity/effective configuration, and perform a saved bridge-readiness run.
   Readiness must be proven by a backend-observed gateway receipt, not model text.
4. Only after readiness passes, construct the kernel `AgentBinding` and commit the
   existing role/assignment transition. Preserve created IDs even if readiness or
   the organization commit fails.
5. For later jobs, verify the current role, assignment, epoch, deadline and grant
   before **each** advance and again before applying results. Preparing a job
   validates its immutable envelope, not live organization authority. Deduplicate
   outbox delivery by finding the same durable operation, never by repeating POST.
6. Stop future dispatch when the role drains/retires, transfer obligations and
   reject late results. The runtime does not change the kernel or a commitment
   ledger. A job result requires a separate domain verifier.

CREAO native completion is especially insufficient for guarded writes: API runs
cannot collect interactive approvals, and an agent with `approval_required` or
`blocked` write autonomy can complete while the action remains unperformed.
[Agent contract](https://docs.creao.ai/developer-api/agents/get-agent)

This hook is coordinated with Claude's ownership of saga/outbox/lifecycle fixes in
issue #4. The core client/runtime and provider adapter are Codex-owned. No direct
patch to Claude's saga or outbox is included in this milestone.

## Bound execution receipts

Domain workflows can call `SpecialistRuntime::execution_receipt(&job, &binding)`
after a native run completes. This reads the configured backend's own CREAO
journal. It never creates an intent, submits a request, polls, retries, or changes
the job. Other specialist implementations reject this operation unless they
implement their own trusted receipt path.

Supply the job and agent binding frozen by the backend at dispatch. The adapter
reconstructs the exact execution specification and matches it against the saved
operation, including organization/account, mission, responsibility, assignment and
epoch, attempt, task/input, deadline, agent ID and configuration digest. The saved
ticket and observation must validate and the operation must be `NativeCompleted`.
Generic job results, provider references and caller-supplied success flags cannot
substitute for that journal evidence.

The binding's creation `conversation_ref` is deliberately excluded: execution
starts a fresh conversation. A receipt also does not attest a required scope;
`Job` does not retain that field. The workflow must freeze and verify its own
required scope against the kernel grant.

The receipt includes native run/agent identities, an operation fingerprint, a
checkpoint digest, and the original output with its digest. It is serializable
for audit storage, but there is no deserialization or public import path that
turns a supplied receipt into trusted evidence. Read it again from the backend
journal when reconstructing a domain proof. Debug output hides provider output;
explicit serialization contains it and belongs only in private backend storage.

A receipt proves the provenance of a completed provider run, not the truth of its
output or any current authority. Missing output is preserved as missing. A domain
workflow must independently validate its output contract and check current mission,
assignment, lease, deadline and grant before applying a result. Historical receipt
reads remain possible after a job terminates or its deadline expires; they do not
authorize a new transition. Native IDs also do not establish a sponsor run share
link or prove a guarded external write occurred.

The venue-continuity workflow will use this boundary to assess verified Apify
observations through the same persistent CREAO agent. This receipt interface alone
does not implement that workflow or resolve the specialist worker findings on
held PR #20.

## Reconfirmed native contracts

- Agent creation accepts natural-language `input` plus optional model and creation
  conversation, returning a Run with an initially nullable agent ID. This client
  accepts matching top-level/result agent identity and rejects conflicting IDs.
  [Create agent](https://docs.creao.ai/developer-api/agents/create-agent)
- Execution accepts an existing agent UUID and JSON input, with optional model,
  conversation and workspace. The client suppresses provider notifications and
  uses polling, not webhook delivery. Input is bounded to 256 KiB including the
  native request envelope, a conservative subset of the documented input limit.
  [Create run](https://docs.creao.ai/developer-api/runs/create-async-run)
- Run statuses are pending, running, completed, failed and cancelled. Unknown
  statuses remain inspectable and unresolved. Result text may be a transcript or
  a serialized session summary, not a verified task result.
  [Run status](https://docs.creao.ai/developer-api/runs/get-run-status)
- No idempotency key for these two POSTs or agent-delete/run-cancel endpoint was
  found in the reviewed contract. The implementation assumes none. Memory updates
  replace content rather than merging, so this milestone does not write memory or
  install account-wide skills automatically.
  [Official OpenAPI](https://docs.creao.ai/developer-api/developer-v1.openapi.yaml),
  [memory replacement](https://docs.creao.ai/developer-api/agent-memory/replace-agent-memory)

## Validation and remaining live gate

Loopback tests exercise real request serialization, response parsing, quota sharing,
failed SQLite commits, POST/GET cancellation, disk recovery, identity drift, unknown
statuses, cross-context ticket rejection and repeated events on one agent. CLI tests
verify offline preparation, strict input, idempotency and credential handling.

No live credentials or paid cloud operations were used. Before a sponsor claim,
create a real agent, persist/poll its run, inspect effective settings, run two distinct
events, execute the generic gateway skill, and retain native IDs plus independently
verified gateway receipts. Agent creation, readiness, execution completion and
responsibility fulfillment must remain separately visible.
