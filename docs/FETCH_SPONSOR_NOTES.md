# Fetch / ASI:One at Cal Hacks 13.0

The [official hackpack](https://www.fetch.ai/events/hackathons/calhacks-13-0/hackpack),
checked October 23, 2026, requires Agentverse registration, Agent Chat Protocol
(ACP), ASI:One discovery/use, meaningful tool or multi-agent execution, and the
primary workflow entirely inside an ASI:One conversation. It also requires a
public GitHub repository. The existing private repository remains private until
the owner authorizes publication. Local signed messages alone do not satisfy
these requirements.

The user's [resource pack](https://docs.google.com/document/d/1pbsW7tH946iooPZxhn2FOuWZ1UjlEfJu4Hi9CQQPBpo)
links a [setup guide](https://docs.google.com/document/d/1_Tb76j7zlzodhTiofHX_uPuEIfrDuQ3QAyk7c4osrd4)
and [submission instructions](https://docs.google.com/document/d/1UDW-X1C24hxZviFOQzjTeh0pXRNAoflMb8lhJqZP9Z0).
Submission goes through both Devpost and the Cal Hacks ASI:One Submission Agent.
Collect the actual Agentverse profile URL, ASI:One shared conversation, demo video,
and runnable source. Do not substitute locally generated identifiers for live links.

## Implemented conversation boundary

`genesis_fetch_bridge.acp` adds standard ACP 0.3.0 to the pinned Python SDK. It is
an optional, separate process and identity alongside the existing private signed
GENESIS organization bridge and venue specialist. The Rust coordinator continues
to own authority, verification, durable effects, child assessment and retirement.
No kernel, planner or UI transition is implemented by the chat adapter.

The recovery adapter supports exact single-text commands:

| Command | Action |
| --- | --- |
| `help` or `cases` | Explain the bounded workflow and list configured demo case names. |
| `recover gala` | Start the operator's frozen `gala` request, or read its known recovery. An uncertain first response can be reconciled by repeating the exact backend request. |
| `status gala` | GET only the known recovery. Before a confirmed ID exists it returns that limitation without starting work. |

A configured case begins with an **already accepted Reopen assessment and already
verified candidate source jobs**. Starting it allocates a recovery responsibility,
recruits the independent Fetch selector, verifies its decision, and requests one
CREAO follow-up assessment through the existing coordinator. The normal organization
worker advances the work; chat is not a polling or orchestration replacement.
Completion returns the selected venue, assessment ID and evidence reference.
It means assessed feasibility, not a booking, closed requirement or completed mission.

This slice enables the recovery subworkflow in a conversation. It does **not** yet
start a new mission, discover/invoke candidate Actors, set up either organization,
or conduct the entire two-organization capability inheritance workflow through
ASI:One. Those are remaining product and sponsor gates, not demonstrated features.

## Primary workflow in ASI:One: report, watch, inherit

`genesis_fetch_bridge.acp_mission` is a second, separate ACP agent that makes the
organization itself usable from an ASI:One conversation. It holds only the
organization's restricted **agent** token: it can record evidence and read state
and events, never commit, provision, publish, book or reset a hold.

| You say | What happens |
| --- | --- |
| `org` | Reads `GET /organization` and summarizes the mission, active and retired roles, what each owns, and requested capabilities. Dev-stub bindings are labeled. |
| `report <what happened>` | Records one observation (`POST /missions/{active}/observations`) with a **fixed** `source` and `evidence_ref` from the agent's configuration; the text is quoted as data. The organization's own planner and kernel decide what changes. The agent then follows the event log for up to `follow_seconds`, relaying planner verdicts, proposed and committed changes, roles created and activated, provisioning (with `DEV STUB` labels), recruitment, capability requests and installations, handoffs and retirements, and ends the session with how to follow up. |
| `status` | Everything after the last report from this agent. |
| anything else | The command list. Nothing is recorded. |

Limits: one in-flight conversation per gateway (others get a busy reply and
nothing is recorded), `max_reports_per_hour` from a 0600 SQLite journal that
also binds identity, backend URL and configuration, 512-byte messages,
loopback-only backend, 10-second HTTP deadline, no redirects, no raw backend
errors in chat. An unconfirmed `POST` is reported, never repeated. The journal
records each report's event cursor so `status` reads only what followed it.

Configuration (`services/fetch-bridge/acp-mission.example.json`): `public_demo`,
`evidence_ref` (the exact https portal page the observation cites, matching the
gap policy allowlist), `source`, `max_reports_per_hour`, `follow_seconds`.
Environment: `GENESIS_ACP_MISSION_SEED` (its own secret, not the recovery
gateway's), `GENESIS_ACP_BACKEND_URL`, `GENESIS_ACP_AGENT_TOKEN`.

```sh
uv run --project services/fetch-bridge --frozen python -m genesis_fetch_bridge.acp_mission identity
uv run --project services/fetch-bridge --frozen python -m genesis_fetch_bridge.acp_mission serve-public \
  --config .local/acp-mission.json --db .local/acp-mission.sqlite \
  --readme services/fetch-bridge/ACP_MISSION_AGENT_README.md --port 8104
```

Then connect it through Inspector → Mailbox exactly as below, confirm it in
ASI:One, and record the profile and shared-conversation URLs. For the planner
to act on a report, `genesis-planner run` must be watching the same
organization with a model configured; without it the chat relays only the
recorded evidence. Tests: `tests/test_acp_mission.py` (loopback backend
fixture, protocol streaming, CLI boundary); no public registration is
performed by tests.

The two-organization inheritance beat is visible from the same conversation
when the second organization's gateway is registered too: `org` on Org B
shows the adopted capability and its version. Multi-agent collaboration is
native to the workflow: the organization recruits an independent Fetch
specialist under a signed contract while the chat relays the contract events.

## Public demo authorization and configuration

`public_demo: true` explicitly makes every listed case callable by **any** ACP
participant and exposes its projected progress/result. Use an isolated demo
organization and non-sensitive public cases. A signed ASI agent address is not an
end-user login or an operator role. The gateway holds an organization operator
token internally because creation is operator-only; chat cannot retrieve it or
supply arbitrary paths, URLs, jobs, deadlines, policies or payloads. It cannot
reset holds, request retries, cancel work, or call generic kernel APIs.

Save this shape in an ignored local file, replacing IDs and the fixed deadline
with actual current evidence before starting the gateway:

```json
{
  "public_demo": true,
  "cases": {
    "gala": {
      "client_request_id": "asi-gala-recovery-1",
      "reopened_assessment_id": "assessment_<real-id>",
      "candidate_job_ids": ["<real-verified-candidate-job>"],
      "deadline": "2026-10-24T01:00:00Z"
    }
  }
}
```

This example is not executable evidence. The Rust source/authority/freshness gates
still apply, including the maximum 30-minute window described in
[venue recovery](VENUE_RECOVERY.md). The gateway never rolls a deadline forward.
There are at most 16 cases, each with 1–8 distinct candidate jobs, and at most one
in-flight backend request per gateway. Extra concurrent commands receive a busy
reply rather than queueing additional backend calls.

Set environment variables locally; do not put values into chat or Git:

- `GENESIS_ACP_SEED`: a separate stable secret, 32–4096 bytes. Do not reuse either
  private bridge participant's seed.
- `GENESIS_ACP_BACKEND_URL`: exact literal loopback URL, for example
  `http://127.0.0.1:4100/api/v1`. Only local organization servers are supported.
- `GENESIS_ACP_OPERATOR_TOKEN`: that demo organization's operator token.

The mode-0600 SQLite journal binds the complete fixed case configuration, backend
URL and public agent identity before HTTP starts. Configuration drift is refused
on restart. Token rotation is permitted; the token is not persisted. Retain the
journal and backend database. Losing a response or crashing before recording the
ID repeats the same backend idempotency key, sources, order and deadline. The
backend's immutable request comparison and per-requirement uniqueness remain the
source of duplicate-effect protection. A mailbox acknowledgement is not a durable
job receipt; the upstream SDK queues messages in memory before processing.

Backend HTTP has a 10-second deadline and 64-KiB response cap, no redirects,
compression or environment proxies. Responses expose a small projection, never
raw backend errors, candidate facts or secrets. No public HTTP server or LLM API
is needed to run the gateway's local tests. ACP itself requires no ASI model API
key; that key is needed only for a handler that calls the ASI model API.

## Explicit public setup

Install the frozen Python environment using the existing
[bridge instructions](../services/fetch-bridge/README.md). Inspect and customize
[the agent profile template](../services/fetch-bridge/ACP_AGENT_README.md), retaining
accurate evidence limits. Its contents will be published, so do not put secrets or
private case configuration in it.

Print the separate public identity without creating an Agent or making network calls:

```sh
uv run --project services/fetch-bridge --frozen python -m genesis_fetch_bridge.acp identity
```

Only the following explicit command constructs an SDK Agent, registers its public
protocol/details and starts public messaging. It also starts Inspector on literal
loopback `127.0.0.1:8103` for account connection; keep that local endpoint private.
The backend's own server must already be running.

```sh
uv run --project services/fetch-bridge --frozen python -m genesis_fetch_bridge.acp serve-public \
  --cases .local/acp-cases.json --db .local/acp.sqlite \
  --readme services/fetch-bridge/ACP_AGENT_README.md --port 8103
```

Follow the [official ASI-compatible agent setup](https://uagents.fetch.ai/docs/examples/asi-1):
open the printed Inspector link, choose **Connect → Mailbox**, associate the
Agentverse account, inspect the agent profile and its ACP protocol, then choose
**Chat with Agent**. Confirm discovery and actual command execution from ASI:One;
keep the process running. Registration logs alone do not prove routing or usability.
Record the real profile and shared conversation URLs after successful execution.

The pinned SDK has no host constructor parameter. This entry point explicitly
sets `uagents.asgi.HOST` to `127.0.0.1` before startup. Inspector also keeps its
server task alive: in 0.26.1, mailbox-only startup with no REST handlers can return
from its main task after spawning polling. Recheck both behaviors on upgrades.
`AlmanacApiRegistrationPolicy` selects API registration instead of ledger
registration transactions. SDK construction still derives a wallet and may perform
contract-version discovery; it is not a promise of zero ledger RPC. Persistent message
history and event reporting are disabled; Inspector retains bounded in-memory
envelope history. SDK concurrent handling lets the gateway return busy while
one backend call is active; it does not impose a network-wide traffic quota. See the pinned
[Agent lifecycle](https://github.com/fetchai/uAgents/blob/v0.26.1/python/src/uagents/agent.py),
[ASGI host](https://github.com/fetchai/uAgents/blob/v0.26.1/python/src/uagents/asgi.py),
[registration](https://github.com/fetchai/uAgents/blob/v0.26.1/python/src/uagents/registration.py),
and [mailbox](https://github.com/fetchai/uAgents/blob/v0.26.1/python/src/uagents/mailbox.py).

## Evidence boundary

Local tests exercise the real ACP models/protocol handlers and the gateway's HTTP
contract against a local fixture, including unknown responses, restart identity,
authority surface restrictions and response bounds. Existing signed native Fetch
process tests and the Rust recovery integration remain separate evidence.
These checks do not register an agent, run the SDK's public lifecycle, test its
Inspector in a browser, establish ASI:One routing, or call live sponsor providers.
The entire primary workflow in ASI:One, public source publication, native provider
runs and submission evidence remain open.
