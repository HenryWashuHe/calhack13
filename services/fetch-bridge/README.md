# Fetch specialist bridge

This process transports GENESIS messages using **native Fetch SDK identities, schema digests, and signed envelopes**. Its SQLite inbox and outbox survive restarts. The optional independent specialist maintains its own contracts for multiple organizations and returns synthetic advisory receipts, not planning recommendations. It never activates a kernel role or verifies a business outcome.

The organization kernel retains authority over recruitment, grants, leases, epochs, assignments, and result acceptance. A bridge `202 queued`, Fetch HTTP `200`, or `task_result` is not domain success. The private API and `GenesisMessage` protocol are GENESIS interfaces, not Fetch-hosted APIs.

## Run

Use Python 3.12 (supported runtime range: 3.11–3.13). This package pins `uagents==0.26.1`, `uagents-core==0.4.12`, Pydantic, and aiohttp; `uv.lock` freezes the full installation.

```sh
uv sync --project services/fetch-bridge --frozen --python 3.12
```

Supply two different environment secrets to each process:

- `GENESIS_FETCH_SEED`: at least 32 bytes from a secure random source, retained across restarts. The same seed produces the same public agent identity. Losing it loses that identity.
- `GENESIS_FETCH_BRIDGE_TOKEN`: a distinct 32–512 character ASCII Bearer token for the local organization-to-bridge API.

Do not place either secret in source, peer JSON, command arguments, or messages. No Agentverse, Apify, Elastic, or CREAO credential belongs in this bridge. `identity` only prints the public address:

```sh
uv run --project services/fetch-bridge --frozen genesis-fetch-bridge identity
```

Create a peer file mapping the public identities printed by the other processes to their signed-ingress URLs:

```json
{
  "<other native agent1 address>": "http://127.0.0.1:8102/submit"
}
```

Each independently operated process uses its own seed, token, SQLite database, and peer file. Then start an organization bridge or a synthetic contractor:

```sh
uv run --project services/fetch-bridge --frozen genesis-fetch-bridge serve \
  --db .local/org-a-fetch.sqlite --peers .local/org-a-peers.json --port 8101

# Run in a separate shell with the specialist's own environment secrets.
uv run --project services/fetch-bridge --frozen genesis-fetch-bridge serve \
  --db .local/specialist-fetch.sqlite --peers .local/specialist-peers.json \
  --port 8102 --specialist
```

The CLI binds only literal loopback. Signed ingress is `POST /submit`; all `/v1/*` API endpoints require Bearer authentication. A remote deployment needs a separately configured TLS ingress exposing **only** `/submit`, while keeping `/v1/*` private. Outbound peers accept HTTPS or literal loopback HTTP; URLs must be preconfigured and cannot contain credentials, queries, or fragments. A search result does not automatically become an allowed peer.

No `Agent` is started, so there is no SDK automatic registration, public status write, telemetry, wallet action, or mailbox polling. Network traffic starts only when a durable outgoing message targets an explicitly configured peer. Public Agentverse discovery and registration remain separate, untested deployment steps.

## Private API

- `GET /v1/identity`: schema version, public address, SDK-derived protocol digest.
- `POST /v1/messages`: `{schema_version:1,recipient,message}`; persist before returning `202` with `state:"queued"`. Same ID/recipient/message repeats return the same acknowledgement, even if delivery has already occurred. A changed retry returns `409`.
- `GET /v1/inbox?after=0&limit=50`: increasing sequence numbers, authenticated transport sender, complete message, and next cursor. `limit` is 1–50; response is at most 1 MiB. The kernel must persist its own cursor transactionally with processing.

`GenesisMessage` contains one strict string field, `wire_json`, holding the full versioned GENESIS message. Its schema digest is `model:3d35410a0bac7663333dda8af6d33f8e7c4d2677da8dd74804a2559574d46e2b`. The SDK protocol digest is `proto:5ff9983d2f971db76bc44c6bb98c9de76f825023ebfff1366d0352a37f2b0756`. Use the same schema at both ends; changes require protocol versioning.

Unknown fields, duplicate JSON keys, nonfinite values, invalid native addresses, invalid IDs, expired new messages, invalid signature/schema/target, and unconfigured senders are rejected. Messages are at most 64 KiB, nested input/output/terms at most 32 KiB, and HTTP ingress at most 256 KiB. JSON nesting is capped at 64 levels; integer tokens must fit signed 64-bit or unsigned 64-bit range, while finite floating-point JSON is supported. Message UUIDs use lowercase canonical form and capability names use ASCII domain-identifier syntax. Authentication is evaluated before reading private request bodies. Transport HTTP requests have deadlines, no redirects, no environment proxy, and bounded response reads.

## Persistence and recovery

SQLite uses full synchronous transactions and a mode-0600 file. A different seed cannot open an existing identity-bound database. Incoming receipt, sender, and canonical message are recorded before `/submit` acknowledges them. An identical replay is acknowledged without adding another inbox item. A changed sender or payload for a recorded message ID conflicts. Native envelope signatures authenticate the sender; all organization context still needs kernel authorization.

Outgoing delivery is **at least once**. A crash after remote acceptance can resend the same application ID; receiving deduplication is mandatory. Failed or ambiguous HTTP attempts retry with bounded backoff until expiry. Expired tasks stop sending even if their enclosing message remains valid. Delivered/expired rows remain evidence. There is no claim of exactly-once execution, native cancellation, or immediate delivery.

Storage stops accepting new evidence at an explicit record/page cap (defaults: 10,000 rows across transport and specialist tables; 64 MiB main SQLite database). It never silently evicts unconsumed receipts. The rollback journal may temporarily require additional disk space. Operators must resolve a full database explicitly; there is no automatic retention/deletion policy. A single active bridge per database is the supported deployment.

The synthetic specialist commits contract/task state, its inbox cursor, and reply outbox in one transaction. An offer must be accepted by its original sender. Sender/org/mission/contract/correlation binding is checked again for each request. Fresh release requests may close an expired lease; acceptance and work still require a live lease. Released contract IDs are not reused. A task's immutable job/attempt and idempotency key cannot be replaced. Releasing Org A's contract leaves Org B's contract usable. Outputs explicitly say `synthetic:true` and `domain_verified:false`; the sample performs no web or external business action.

## Validation

```sh
uv run --project services/fetch-bridge --frozen python -m unittest \
  discover -s services/fetch-bridge/tests -v
```

The integration test starts **three separate OS processes**: Org A, Org B, and one independent specialist. It exercises signed offer/accept/task/result messages over HTTP, an offline durable queue, sender and specialist restarts, receiver replay deduplication, stable identity, and release isolation. It does not use Bureau's same-process dispatcher shortcut. Other tests cover tampering, forged identity/target/schema, expiry, immutable retry conflicts, capacity, private authentication, bounded parsing, and redirect refusal. No public credentials, registration, or authenticated cloud smoke test is claimed.

## Verified upstream contracts

- [uAgents 0.26.1 release and Python support](https://pypi.org/project/uagents/0.26.1/), [core 0.4.12](https://pypi.org/project/uagents-core/0.4.12/).
- [Native envelope signing](https://github.com/fetchai/uAgents/blob/v0.26.1/python/uagents-core/uagents_core/envelope.py): sender, target, session, schema, payload, expiry and nonce are signed. Outer `version` and `protocol_digest` are not, so they cannot confer authority; the GENESIS version/context is inside the signed payload.
- [SDK model schema](https://github.com/fetchai/uAgents/blob/v0.26.1/python/uagents-core/uagents_core/models.py): the SDK model uses Pydantic v1 compatibility.
- [SDK agent startup](https://github.com/fetchai/uAgents/blob/v0.26.1/python/src/uagents/agent.py), [ASGI server](https://github.com/fetchai/uAgents/blob/v0.26.1/python/src/uagents/asgi.py), [transport](https://github.com/fetchai/uAgents/blob/v0.26.1/python/src/uagents/communication.py): default startup/HTTP behavior is why this service wraps the native signing primitives with explicit bounded networking.
- [Current Agentverse search tutorial](https://innovationlab.fetch.ai/resources/docs/agentverse/searching), [v1 API reference](https://docs.agentverse.ai/v-1/api-reference/search/agents). The v1 reference is marked deprecated while the tutorial still targets `/v1/search/agents`; no replacement endpoint is assumed here.
