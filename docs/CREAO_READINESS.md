# CREAO readiness probe

The kernel gateway and the CREAO runtime now connect through
`creao_readiness::CreaoReadinessProbe`. When the saga has an app-bound role,
the gateway issues a short-lived challenge and the probe submits one durable run
to that exact published app. Only the callback received by the gateway can establish
readiness. A run ID, native completion or agent-generated success report cannot.

## Enable on an organization server

Configure the existing CREAO runtime variables in the backend environment:

```sh
GENESIS_CREAO_DB=/private/genesis/org-a-creao.sqlite
GENESIS_CREAO_ACCOUNT=shared-creao-account
GENESIS_CREAO_QUOTA_DB=/private/genesis/creao-account-quota.sqlite
CREAO_API_VERSION=trigger
CREAO_TRIGGER_API_KEY=<app-scoped capi_ key from your secret store>
CREAO_TRIGGER_APP_ID=<published app UUID>
GENESIS_PUBLIC_URL=https://org-a.example.com
```

Create the private parent directory first. Keep the key out of source control and
command arguments. `genesis org` reads its environment; it does not implicitly
load a `.env` file. A complete runtime configuration without `GENESIS_PUBLIC_URL`
keeps the existing behavior: bound roles remain awaiting readiness. Setting
the public URL enables the probe and gateway verifier; partial CREAO settings or
invalid URLs refuse startup with sanitized errors. HTTPS is required except for
local test loopback URLs. Each organization needs its own reachable callback URL
and journal; organizations on one CREAO account share its quota database and
namespace.

Startup recovers the runtime once, then the specialist and probe share clones of
the same runtime gate and account limiter. Neither startup nor registration makes
a provider request. The next authorized provisioning pass can create a paid run.

## Provider protocol and recovery

- `OperationPurpose::Readiness` stores the challenge ID, role configuration digest
  and expiry separately from provisioning and execution. The full immutable
  request also binds organization, account, role, agent, nonce and callback URL.
- `POST /api/v1/apps/{appId}/runs` targets the configured app with an `inputs`
  object and no shared conversation. Input identifies the
  `genesis_gateway_readiness` task and carries the exact receipt JSON. The
  published app must already support the GENESIS gateway callback; API Trigger
  cannot install a skill or create an agent. Bootstrap is not evidence of either.
- The runtime persists the request before submission. `submit_prepared` checks
  under the shared gate that the operation is still prepared and unexpired;
  concurrent dispatch cannot turn a second call into a polling GET. Replays with
  a saved ticket return the same dispatch reference without provider I/O.
- The adapter never polls, retries a POST, or treats native status as readiness.
  Timeout, cancellation, attempted POST failure and checkpoint-save uncertainty
  remain unknown. Recovery retains the saved intent and never replays it. A new
  kernel challenge after expiry is a separate bounded attempt.
- Generic specialist `get`/`advance` cannot reinterpret readiness operations as
  role creation or mission output. Expired records remain readable for diagnosis,
  but an expired prepared readiness operation cannot be submitted.

The request uses the documented [API Trigger contract](https://docs.creao.ai/pro/api-trigger)
and the runtime's conservative shared local limiter.

## Evidence and private data

The gateway challenge table stores only a hash of the nonce. The separate private
CREAO journal stores the exact run input, including the plaintext nonce, to retain
immutable intent and diagnose interrupted work. CREAO also receives that input,
which is account-visible. Keep the journal and its WAL/sidecar files private;
request/observation Debug output redacts inputs and provider output. Public
challenge/status responses and errors must never expose the nonce.

A valid receipt establishes that a holder of the challenge reached this kernel
within its lifetime and named the bound agent. It does not establish mission
competence, effective agent configuration, an installed skill or a guarded
business action. Configure and test the callback in the published app before
claiming live readiness. API Trigger does not expose Developer skill metadata.

## Verification scope

Current Trigger fixtures add exact app/path dispatch and completion-without-
callback rejection. The following counts and notification-suppression assertions
are historical Developer-protocol checks, retained as regressions. They do not
claim a live Trigger readiness run.

`tests/creao_readiness_probe.rs` exercises the real Rust client/runtime and gateway
HTTP route against a loopback provider: exact inputs and notification suppression,
concurrent dispatch, changed bindings, expiry, ambiguous/cancelled POST recovery,
SQLite reopen, native completion without readiness, a callback arriving before
an ambiguous response, and a successful POST followed by a failed receipt save.
Configuration tests cover explicit registration and missing settings;
runtime/provider tests cover purpose isolation and secret-safe Debug, including
a provider echoing the nonce in model metadata.

At code checkpoint `df1d285`, all 407 Rust workspace tests (53 suites, zero failed
or ignored) pass with `serde_json/preserve_order`, along with formatting and
strict default-feature all-target Clippy. This is the focused branch based on
#26; the broader combined integration branch has a separate validation record.

The prior integration candidate combined main `ec5ec222`, gateway #26 at
`9935bd8`, attach #27 at `29c9853`, and this provider slice at `c1e6ba4`. The original
draft's gateway correlation and specialist-authority blockers are addressed in
those dependencies. All 122 focused checks pass with `serde_json/preserve_order`:
50 CREAO client/runtime/CLI/limiter tests, 13 provider tests, 57 server integration
tests (including all eight probe regressions), and two configuration unit tests.
Formatting and strict default-feature workspace/all-target Clippy also pass.
Current main handoff documents remain authoritative for merge status; this is a
candidate validation record, not a claim that the candidate has merged.

After main `e2563ef` added the gap-acquisition driver and attention UI, its merge
into this candidate was conflict-free. All 40 affected default-feature checks
pass: 16 gap-driver cases, eight CREAO probe cases, 14 CLI tests and two CREAO
configuration tests. Formatting and strict default-feature workspace/all-target
Clippy pass. The previously reviewed CREAO provider/runtime sources are unchanged;
main's gap startup, policy flags, routes and UI are preserved.

No live CREAO account or installed provider skill was exercised. The candidate
establishes local protocol and recovery behavior. An actual skill installation,
reachable callback, agent ID and genuine run share link still need live evidence;
readiness itself does not fulfill the separate core mission-work requirement.
