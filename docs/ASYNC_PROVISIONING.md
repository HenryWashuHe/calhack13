# Asynchronous role provisioning (pending/readiness hook)

Implements the kernel side of the hook described in
[CREAO_RUNTIME.md](CREAO_RUNTIME.md) (steps 1–4). Code:
`crates/genesis-server/src/specialist.rs`, called from `saga::run`.

## Behavior

For a role whose provider is registered with `ProviderSet::with_specialist`
(a durable `SpecialistRuntime`, e.g. `CreaoSpecialist`), the saga:

1. **Prepares or finds** the role's operation (`prepare_role`, idempotent for
   the same organization/change/role) and records it on the change as
   `OrgChangeRecord.operations[role_key]`.
2. **Advances at most once per pass**, outside the kernel lock, bounded by the
   provisioning timeout. A failed read is retried once with `retry_read`; the
   runtime never replays a POST. The observed state is persisted:
   - `pending`: the change stays `PROVISIONING`; the organization, including
     existing responsibility ownership, is unchanged.
   - `needs_reconciliation`: kept visible with an attention message; **no
     automatic retry**. An operator must reconcile the saved operation.
   - `failed`: the change fails; the operation and run IDs are kept in events.
3. On native completion, **persists the created agent binding** (and a
   `role_provisioned` event) before anything else, then moves to
   `awaiting_readiness`.
4. Asks the provider's **`ReadinessVerifier`** for backend-observed evidence
   (e.g. a bridge/gateway receipt). Only `Ready { evidence_ref }` lets the role
   activate; the reference is stored on the operation. `NotReady` fails the
   change and keeps the agent ID. **Without a registered verifier, a created
   agent stays in `awaiting_readiness` and the role never activates.** Provider
   completion and model text are not readiness.

Every state change emits a `provisioning_progress` event (state, operation ID,
run ID, sanitized attention). A background poller (`specialist::spawn`, every
500 ms, only when a specialist is configured) re-runs changes with pending
operations; the runtime honors its own poll due times. Restarts resume from the
saved operation without preparing a new one. The per-change saga gate
serializes drivers.

The console shows the operation state, run ID and attention on hatched pending
roles, and describes `provisioning_progress` events in the evidence log.

## Jobs for asynchronous specialists

`crates/genesis-server/src/specialist_jobs.rs` implements CREAO_RUNTIME.md
steps 5–6 on the kernel side:

- An outbox `job_dispatch` for a role served by a specialist runtime checks
  live authority and prepares one durable job operation (`prepare_job`,
  deterministic per job attempt). Redelivery is a no-op: it never causes a
  second POST.
- The specialist poller advances each running job at most once per pass.
  Live authority (active mission and role, open responsibility, exact current
  assignment and epoch, mission/job deadlines, assignment lease, matching
  provider binding) is checked **before preparation, each advance and any read
  retry**, and again **before a
  collected result is applied**. Losing authority stops advancing
  (`RESULT_REJECTED`, "authority lost before advance"); a result collected
  after a handoff is archived (`RESULT_REJECTED`, "late result fenced").
- Collected output is stored with `domain_verified: false` and the job moves
  to `VERIFYING`, never `SUCCEEDED`; no commitment is recorded. An unknown
  submission outcome moves the job to `UNKNOWN`; a failed operation to
  `FAILED`, keeping the run ID.
- The generic `/jobs/{id}/results` endpoint refuses CREAO jobs from creation,
  including after a restart without CREAO configured. Generic `/jobs` dispatch
  cannot override a CREAO role's provider to bypass that guard. Backend
  capability routes can still prepare Apify acceptance and invocation jobs for
  that role after validating its installed Actor and authority.

The shared authority check does not revalidate an original required scope:
generic jobs do not persist that field. Native completion is still not domain
success, and the provider binding check here compares the provider, not a frozen
agent/configuration receipt. Domain finalizers must use the bound execution
receipt described in [CREAO_RUNTIME.md](CREAO_RUNTIME.md).

Tests: `crates/genesis-server/tests/specialist_jobs.rs`,
`review_specialist_authority.rs` and `capability_invocation.rs`.

## Still needed before a live CREAO role

- **Registration is done** (`creao_config::attach_creao_from_env`, called at the
  start of `serve_org`). Set all four of `GENESIS_CREAO_DB`,
  `GENESIS_CREAO_ACCOUNT`, `GENESIS_CREAO_QUOTA_DB` and `CREAO_ACCOUNT_API_KEY`
  in the backend environment; the server does not implicitly load `.env`.
  Leaving all settings unset keeps CREAO unconfigured. A partial set refuses
  startup and names the missing variables (never the key).
  Startup runs `recover_interrupted()` before serving. `/health` lists
  `configured_specialists`. Being registered is not being ready: without a
  readiness verifier a created CREAO agent stays `awaiting_readiness`.
- **The readiness verifier's kernel side is done**: `GatewayReadiness` issues a
  one-time challenge and accepts the agent's receipt at
  `POST /api/v1/gateway/readiness-receipts` (see
  [GATEWAY_READINESS.md](GATEWAY_READINESS.md)). The CREAO provider probe and its
  distinct durable readiness purpose are implemented. Set `GENESIS_PUBLIC_URL`
  along with the complete CREAO configuration to register it. Actual skill
  installation and a live-account receipt remain unverified; see
  [CREAO_READINESS.md](CREAO_READINESS.md).
- **Reconciling a lost creation is done.** For a `needs_reconciliation`
  operation an operator (operator token) can either:
  - **attach** the native agent found in the provider account:
    `POST /api/v1/org-changes/{id}/operations/{role_key}/attach` with
    `{"provider_agent_id": "...", "reason": "..."}`. The binding uses the
    configuration digest of the role's original creation request, the
    operation moves to `awaiting_readiness` with
    `native_status: "operator_attached"` (no `role_provisioned` event: the
    provider never reported this agent), and the role activates only after
    the readiness verifier passes for that agent ID (for CREAO, a gateway
    receipt naming it). No provider request is made and nothing is
    re-created. Refused with `409 operation_not_reconcilable` unless the
    outcome is unknown and no agent is bound yet, and with
    `409 agent_already_bound` if the ID is bound to any role or to another
    change. A receipt proves a holder of the run's challenge reached GENESIS
    and named that agent. It does not prove the installed skill or effective
    configuration matches the role. That stays the operator's assertion,
    recorded with the reason.
  - **abandon** it:
    `POST /api/v1/org-changes/{id}/operations/{role_key}/abandon` with
    `{"reason": "..."}` marks the operation failed and ends the change FAILED,
    keeping every provider ID; nothing is retried.
- A **domain verifier** that can move a `VERIFYING` job to `SUCCEEDED`, and
  draining/retirement handling of jobs still running when a role retires
  (today they are fenced on the next advance).

Attach tests: `crates/genesis-server/tests/operation_attach.rs`.

Tests: `crates/genesis-server/tests/specialist_readiness.rs` (scripted runtime
and verifier: pending → created → ready, no verifier, ambiguous creation,
failed operation, failed readiness, transient read errors, restart, and the
server poller).
