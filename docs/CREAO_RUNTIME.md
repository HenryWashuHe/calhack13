# CREAO API Trigger runtime

Production server, `genesis-creao` execution CLI and the CREAO live gate use
app-scoped API Trigger. They do not call Developer v1/v2 or fall back to an
account/project key. The API Trigger credit pool is managed by CREAO; configuration
alone does not prove authentication, billing balance or a completed live run.
See the [official contract](https://docs.creao.ai/pro/api-trigger) and
[API comparison](https://docs.creao.ai/developer-api/api-paths).

## Configuration

Publish a GENESIS-capable app on `agent.creao.ai` first, then configure:

```dotenv
CREAO_API_VERSION=trigger
CREAO_TRIGGER_API_KEY=<app-scoped capi_ key from private storage>
CREAO_TRIGGER_APP_ID=<published app UUID>
# Optional; if present, it must identify this exact app on the official host.
CREAO_TRIGGER_RUNS_URL=https://agent.creao.ai/api/v1/apps/<published app UUID>/runs
GENESIS_CREAO_DB=data/creao-trigger/org-a.sqlite
GENESIS_CREAO_ACCOUNT=creao-trigger-team
GENESIS_CREAO_QUOTA_DB=data/creao-trigger/team-quota.sqlite
```

Create private parent directories. The organization server reads its process
environment; it does not implicitly load `.env`. Use your existing private env
loader. Keep keys out of source, command arguments, logs and frontend configuration.
An absent/blank version defaults to Trigger. Explicit `v1` or `v2` refuses
execution. `CREAO_ACCOUNT_API_KEY` and `CREAO_V2_API_KEY` are not substitutes.
A partial configuration refuses startup; no settings leaves CREAO disabled.

Each organization needs its own NEW operation journal. Organizations sharing the
same team quota use the same quota database and explicit account namespace.
Keep old Developer journals intact for offline inspection; never relabel their
saved operations as Trigger. The optional `GENESIS_PUBLIC_URL` enables the
[readiness gateway probe](CREAO_READINESS.md). Without a reachable callback and a
published app capable of submitting it, a role stays awaiting readiness.

## Role and job semantics

API Trigger can run an existing published app; it cannot create/edit cloud
agents, install skills or read Developer agent metadata. GENESIS still creates,
assigns and retires organizational roles in its kernel. `prepare_role` binds
those roles to the configured app through a real bootstrap run carrying the
full role contract and organization/change/role/configuration identity. It does
not claim to create a new CREAO app or persist the role into shared app state.

For compatibility, specialist progress calls a completed bootstrap `Created`,
with `readiness_verified:false`. Only a separate challenge-bound gateway receipt
can establish readiness. Bootstrap completion does not activate a role or prove
the app's instructions/skills are installed. The published app must already
understand GENESIS task envelopes and support the gateway callback.

Each job resends the exact role contract from a completed local bootstrap, pinned
by operation identity and fingerprint. Missing or ambiguous matching bootstraps
are refused; an already prepared job retains its original bootstrap on replay.
It also sends its frozen task, input and complete organization, mission, role,
responsibility, assignment, epoch, job, attempt and configuration context as
`inputs`. It does not resume a bootstrap conversation. Multiple roles may bind
the same app: isolation comes from kernel authority and per-operation context,
not separate provider apps. This API is not a physical multi-tenant sandbox.

## Wire and recovery guarantees

- POST `https://agent.creao.ai/api/v1/apps/{appId}/runs` with `{"inputs":{...}}`
  and app-scoped Bearer authorization. Only the exact configured app and official
  URL are accepted. No redirects, transparent retries or proxy routing.
- POST receipts require exact `appId` and run `id`. Polling uses only
  `GET /api/v1/apps/{appId}/runs/{runId}`. A GET may omit `appId` because both
  identities are pinned in the URL and retained ticket; a conflicting ID fails.
- Trigger is a distinct persisted API version and request fingerprint including
  the app path. Foreign apps and old protocols cannot be advanced or recovered
  with new configuration. Unsupported create-agent/model/workspace/conversation
  request options are rejected before HTTP instead of silently discarded.
- The SQLite journal commits immutable intent before one POST. Lost/ambiguous
  POST receipts require reconciliation, never a blind resend. Known tickets
  resume with GET only. Poll failures need explicit read retry. Runtime guards
  recheck authority/deadline before network effects.
- Response size, JSON ambiguity, output provenance and credential echoes remain
  checked. Shared local quota reservations/cooldowns persist across restarts.
  The default limiter is conservative; it is not a promise about Trigger billing
  or server-side quotas. An observed 429 extends the shared cooldown.

The browser-compatible User-Agent matches the existing Trigger smoke's observed
CDN GET workaround. It is not an authentication fallback.

## Backend CLI

Copy the example to a private input file and replace its app UUID/account with
the configured values. Preparation and status make no network calls:

```sh
cargo run -p genesis-creao -- prepare --db data/creao-trigger/org-a.sqlite \
  --org org_a --spec crates/genesis-creao/examples/trigger-role.json
cargo run -p genesis-creao -- status --db data/creao-trigger/org-a.sqlite \
  --org org_a --operation demo-trigger-role-1
```

With the private configuration loaded, advancing sends one potentially billable
operation per call:

```sh
cargo run -p genesis-creao -- step --db data/creao-trigger/org-a.sqlite \
  --org org_a --operation demo-trigger-role-1 --account creao-trigger-team \
  --quota-db data/creao-trigger/team-quota.sqlite
```

Respect `next_poll_at`; `--retry-read` can retry GET, never POST. Startup
`recover` takes the same db/org/account/quota arguments and makes no provider
request. `status` omits private inputs and extracted output. Keep SQLite journals
and their sidecar files private and use one active runtime per organization.

Historical v1/v2 specs can still be prepared/inspected offline. Legacy explicit
library constructors and their protocol tests remain for historical records;
production entry points no longer select them. `reconcile-agent` is disabled in
this CLI and cannot call the old Developer edit API. Preserve uncertain legacy
checkpoints and inspect the original provider's records independently.

## Execution evidence

`SpecialistRuntime::execution_receipt` reconstructs a completed run from the
backend's journal and exact frozen job/binding. It performs no network action
and cannot import a caller-supplied receipt as trusted evidence. Request,
operation, checkpoint and output hashes retain the native app/run identity.
Missing output remains missing. Native completion proves run provenance, not
semantic correctness, current authority, successful external writes or recursive
improvement; domain verification and current grant checks remain separate.

Synthetic HTTP/SQLite tests cover Trigger contracts, foreign apps/protocols,
recovery, no-repeat POST, authority checks, provider role/job separation and
readiness boundaries. These are local tests, not live CREAO or sponsor evidence.
Historical Developer runs and API Trigger smoke reports remain distinct.
