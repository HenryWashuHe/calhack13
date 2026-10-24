# CREAO readiness probe

The kernel gateway and the CREAO runtime now connect through
`creao_readiness::CreaoReadinessProbe`. When the saga has a created agent binding,
the gateway issues a short-lived challenge and the probe submits one durable run
to that exact agent. Only the callback received by the gateway can establish
readiness. A run ID, native completion or agent-generated success report cannot.

## Enable on an organization server

Configure the existing CREAO runtime variables in the backend environment:

```sh
GENESIS_CREAO_DB=/private/genesis/org-a-creao.sqlite
GENESIS_CREAO_ACCOUNT=shared-creao-account
GENESIS_CREAO_QUOTA_DB=/private/genesis/creao-account-quota.sqlite
CREAO_ACCOUNT_API_KEY=<account key from your secret store>
GENESIS_PUBLIC_URL=https://org-a.example.com
```

Create the private parent directory first. Keep the key out of source control and
command arguments. `genesis org` reads its environment; it does not implicitly
load a `.env` file. A complete runtime configuration without `GENESIS_PUBLIC_URL`
keeps the existing behavior: created agents remain awaiting readiness. Setting
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
- `POST /v1/runs` uses the created agent ID, a fresh conversation and no workspace.
  The client forces notification suppression. Input identifies the
  `genesis_gateway_readiness` task, carries the exact receipt JSON and describes
  the narrow readiness operation. The creation instructions request a gateway
  skill and distinguish its nonce-authorized receipt from mission-job authority.
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

The request uses the existing documented [CREAO run API](https://docs.creao.ai/developer-api/runs/create-async-run)
and shared [account limits](https://docs.creao.ai/developer-api/rate-limits).

## Evidence and private data

The gateway challenge table stores only a hash of the nonce. The separate private
CREAO journal stores the exact run input, including the plaintext nonce, to retain
immutable intent and diagnose interrupted work. CREAO also receives that input,
which is account-visible. Keep the journal and its WAL/sidecar files private;
request/observation Debug output redacts inputs and provider output. Public
challenge/status responses and errors must never expose the nonce.

A valid receipt establishes that a holder of the challenge reached this kernel
within its lifetime and named the bound agent. It does not establish mission
competence, effective agent configuration, a particular installed skill, or a
guarded business action. CREAO has no per-run skill-selection field; inspect the
actual agent's [skills](https://docs.creao.ai/developer-api/skills) and test them in
the intended account before claiming a live integration. Installing the skill is
requested in creation instructions, not asserted as verified by the backend.

## Verification scope

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

No live CREAO account or installed provider skill was exercised. This slice is
stacked on kernel gateway PR #26 and does not resolve that PR's early optional-run
correlation finding or the outstanding #20 managed-job authority findings. Those
source fixes remain required before the assembled system is ready to merge.
