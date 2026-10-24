# Gateway readiness receipts

An asynchronously created agent (CREAO) is **not ready** because its creation
run completed, or because the agent says so. GENESIS treats a role as ready
only when the backend itself observes the created agent calling back through
the GENESIS gateway. This document covers the kernel side, which is
implemented, and the provider-side probe, which is not.

Code: `crates/genesis-server/src/gateway_readiness.rs`, routes in
`crates/genesis-server/src/routes/gateway.rs`. It plugs into the
pending/readiness hook described in [ASYNC_PROVISIONING.md](ASYNC_PROVISIONING.md)
as the provider's `ReadinessVerifier`.

## Flow

```
saga pass (role created, awaiting_readiness)
  └─ GatewayReadiness::verify
       ├─ issue challenge: challenge_id + random nonce, store sha256(nonce)
       └─ ReadinessProbe::dispatch(ProbeRequest)       ← provider side (CREAO run)
                                    │
           created agent's gateway skill
                                    │  POST /api/v1/gateway/readiness-receipts
                                    ▼  {"challenge_id","nonce","provider_agent_id"}
       kernel accepts once (live, matching nonce + agent ID)
next saga pass
  └─ verify → Ready { evidence_ref: "gateway-receipt:{challenge_id}" } → role commits
```

1. **Challenge.** For the exact created agent (organization, role, provider
   agent ID, configuration digest) the kernel saves a challenge with a random
   nonce (two UUIDv4s, 244 random bits). Only the nonce's SHA-256 is stored.
2. **Probe.** A provider-specific `ReadinessProbe` asks that agent to run its
   gateway skill with the challenge. It is called **at most once per
   challenge**.
3. **Receipt.** The skill posts the challenge back. The kernel accepts it only
   for a live challenge whose nonce and agent ID match, once.
4. **Readiness.** The next provisioning pass (the poller runs every 500 ms)
   sees the receipt and reports `Ready`; the change commits the role and the
   operation's `readiness_evidence` is `gateway-receipt:{challenge_id}`.

## Failure handling

| Situation | Result |
| --- | --- |
| Probe accepted | Challenge `dispatched`; waits up to `ttl` (default 5 min) for the receipt. |
| Probe outcome unknown (`ProviderError::Unknown` / `Timeout`) | Challenge stays `dispatching` and **still accepts a receipt until it expires**; no second dispatch. |
| Probe definitely failed (any other error) | Challenge `dispatch_failed` (its nonce never left the backend, so its receipt is refused); the next challenge waits `retry_after` (default 15 s), doubling per failure up to 10 min. These do **not** count toward `max_attempts`. |
| Receipt arrives while the probe call is still in flight | Accepted provisionally; whatever the probe then reports, the challenge stays `received` (status changes after dispatch only apply to a challenge still `dispatching`). The run the receipt names is kept separately (`receipt_probe_ref`) and must match the run the dispatch reports (`probe_ref`, trusted); a mismatch voids the receipt and a new challenge is issued. A receipt naming no run, or the same run, counts. |
| No receipt before expiry | Challenge `expired`; a late receipt gets `410`; a new challenge is issued. |
| `max_attempts` (default 3) challenges that may have reached the agent got no receipt | Readiness fails: "no gateway readiness receipt after N challenge(s)". Created provider IDs are kept. |
| `max_dispatch_failures` (default 5) definite dispatch failures | Readiness fails: "readiness probe could not be dispatched after N attempt(s)". |
| Process restart | Challenges and receipts are in the organization's SQLite store; a stored receipt still proves readiness and no new challenge is issued. |

While it waits, the operation's attention line names the live challenge and
its expiry (for example "waiting for the agent's gateway receipt (challenge
rc-…, expires 18:42:10 UTC)"), so operators can inspect it with the route
below. Probe errors are reduced to their kind and HTTP status; a provider's
message or response body is never stored, logged or shown, since it could echo
the run input that carries the nonce.

## HTTP contract for the gateway skill

```
POST {receipt_url}                      (e.g. https://genesis.example.com/api/v1/gateway/readiness-receipts)
Content-Type: application/json

{"challenge_id": "rc-…", "nonce": "…", "provider_agent_id": "…"}
```

`probe_ref` (the provider run ID) may be added if the skill knows it; it must
match the run the probe reported. No other fields are allowed and the body is
limited to 4 KiB.

| Response | Meaning |
| --- | --- |
| `200 {"challenge_id": "rc-…", "duplicate": false}` | Accepted. A repeat of the same receipt returns `duplicate: true` and changes nothing. |
| `403 {"error": "invalid_receipt", …}` | Unknown challenge, wrong nonce, wrong agent or wrong run. These cases are indistinguishable. |
| `410 {"error": "challenge_expired", …}` | Valid receipt for an expired challenge. The kernel issues a new one while attempts remain. |
| `413`, `415` or `422` (plain text) | Oversized, non-JSON or malformed body, rejected before any lookup. |

The route takes **no bearer token**: provider skills run in account-wide
sandboxes that must not hold organization tokens. The one-time nonce is the
credential. Operators can inspect a challenge (never the nonce or its hash):

```
GET /api/v1/gateway/readiness-challenges/{challenge_id}     (operator token)
```

## Provider-side probe (not implemented; CREAO side)

The kernel calls:

```rust
#[async_trait]
pub trait ReadinessProbe: Send + Sync {
    async fn dispatch(&self, request: &ProbeRequest) -> Result<ProbeDispatch, ProviderError>;
}
```

`ProbeRequest` carries `challenge_id`, `nonce` (secret; its `Debug` output is
redacted), `receipt_url`, `provider_agent_id`, `config_digest`, role IDs and
`expires_at`. `request.expected_receipt()` is the exact JSON the skill must
post. For CREAO, a probe would:

1. Prepare a durable run on the created agent (`request.provider_agent_id`).
   This needs a readiness purpose in the CREAO runtime (today it only has
   `Provision` and `Execution`), with an operation ID derived from the
   organization and `challenge_id`.
2. Put the receipt URL and `expected_receipt()` in the run input and instruct
   the agent's generic gateway skill to POST it and report the HTTP status.
3. Submit once (one runtime step) and return `ProbeDispatch { probe_ref:
   Some(run_id) }`. Map a POST that may have run to `ProviderError::Unknown`.
4. Not decide readiness from the run's completion or text. The kernel decides
   from the receipt alone.

Registration, once a probe exists (for example in `creao_config`):

```rust
let cfg = GatewayConfig::from_public_url(&std::env::var("GENESIS_PUBLIC_URL")?)?;
let app = gateway_readiness::attach(app, Provider::Creao, Arc::new(probe), cfg)?;
```

`GENESIS_PUBLIC_URL` is the base URL at which the provider's sandbox reaches
this organization's kernel. It must be `https` (plain `http` is accepted only
for loopback, for tests) and carry no credentials, query or fragment.

## What a receipt proves, and what it does not

- It proves that, within the challenge lifetime, something holding that run's
  input reached this kernel's gateway and named the created agent. In the
  intended path that is the agent's own skill, which shows the skill is
  installed, runs, and can reach GENESIS over the network.
- The nonce is visible in the provider run input (account-wide in CREAO). It
  is single-use, short-lived and bound to one agent ID, so a replay can at
  most confirm that same challenge.
- It does **not** prove that the agent can do the role's work or that any
  guarded write happened. Job results still need a domain verifier, and the
  agent still needs a live assignment and grant for every job.

## Tests

`crates/genesis-server/tests/gateway_readiness.rs`, against a real HTTP
listener and a scripted probe:

- the receipt commits the role, with a single dispatch and visible evidence,
  and a replayed receipt changes nothing;
- forged receipts (wrong nonce, agent, challenge or run) all get the same
  `403`, and unknown fields or oversized bodies are refused;
- expired challenges are reissued with a new nonce, then readiness fails after
  `max_attempts`, keeping the created agent ID;
- an unknown dispatch outcome keeps the challenge live without re-dispatching,
  and a late receipt still counts;
- a definite dispatch failure backs off, then retries with a new challenge;
  failures do not use up receipt attempts, and persistent failures fail
  readiness with their (sanitized) cause;
- a receipt accepted while the probe call is still in flight survives any
  probe answer (accepted, unknown or failed);
- a stable waiting note emits no repeated progress events;
- a stored receipt survives a restart.

No live provider was contacted; the probe in the tests is scripted.
