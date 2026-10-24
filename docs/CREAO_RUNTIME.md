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
  creation conversations are not silently reused for execution. Created receipts
  expose `readiness_verified:false`; execution receipts expose
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
