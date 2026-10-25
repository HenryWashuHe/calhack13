# Invention lab: the ASI:One conversation

**Status: implemented capability-chat surface plus proposed development-method
views.** Reconciled against main `0643c9efaee90042b5fc0025a8e1f8e3264a2f50`.
GENESIS targets networked recursive capability improvement with fixed model
weights. Chat reports organizational needs and exposes recorded state; it does
not itself establish a development-method loop or measured improvement.

The adapter is [acp_mission.py](../../services/fetch-bridge/genesis_fetch_bridge/acp_mission.py).
Sender-scoped durable reporting/status (#3) and capability commands, interactive
cards and local chat (#5) are merged. Public Agentverse/ASI:One routing remains
unverified here; see [Fetch setup and evidence limits](../FETCH_SPONSOR_NOTES.md).

## Authority boundary

The gateway holds the organization's restricted **agent** token. Its commands
read state/events or record one observation. They cannot directly commit a
change, provision a role, publish/install a capability or method, start an
experiment or adoption, spend a grant, or reset an operator hold. A report can
prompt the organization's own planner and kernel to act under their existing
authority; recording it does not guarantee a structural change.

The signed ACP sender and message ID are journaled before the observation POST.
A redelivery does not repeat that effect; two senders reusing an ID are distinct
reports. Status uses a durable per-sender cursor. The terminal stand-in uses a
local identity rather than authenticated ACP delivery. Neither mode makes the
sender an organization operator.

## Implemented commands

Paths below are relative to the configured backend API base. These are the
current commands, not proposed method-registry APIs.

| You say | What is read or recorded | What the reply can establish |
| --- | --- | --- |
| `help` | No backend effect | The bounded command list and authority limits. |
| `org` (also `lab`, `who`) | `GET /organization` | Recorded mission, roles, responsibilities and requested capabilities. |
| `report <what happened>` | One `POST /missions/{active}/observations` with configured source/evidence reference, then `GET /events/log` | The report's recorded observation and correlated planner/change events. It does not prove the report is true or that a change succeeded. |
| `status` | `GET /events/log`, `GET /organization` | Bounded pages after this sender's last report; the durable cursor advances past displayed or non-rendering events. |
| `methods` (also `capabilities`) | `GET /organization` installations and role capability attachments | Installed **capabilities** by `id@version`, installation status and holders. This does not list executable development methods from the new registry. |
| `lineage <capability-id>` | Local installations/attachments, then `GET /capabilities/{id}/versions/{version}` | Manifest publisher/verification and this organization's installation state. It does not traverse method derivation, search the method registry or read candidate adoption decisions. |

Unknown commands return help without recording an observation. In particular,
`challenge`, `outcome`, `experiment`, `compare` and `declined` are not commands.
The current capability replies contain historical wording about methods not
being an artifact kind and manifest derivation not being recorded. That wording
is a limitation of this capability projection: separate development-method
contracts and registry support now exist, but are not connected to chat.

## Interactive cards and evidence labels

Merged replies can include ASI Interactive Cards (`DetailCardPayload` in
`MetadataContent`). Buttons carry only `help`, `org`, `status`, `methods` or a
validated `lineage <id>` command. A card cannot submit a report; an ambiguous
message containing both text and a card response is ignored. Real ASI:One card
rendering is not established by local handler tests.

Dev-stub bindings remain labeled `DEV STUB`; fixtures are not live providers.
A recorded planner decision names its recorded model. Unknown or missing
outcomes remain uncertain. A manifest's publisher verification is distinct from
the receiver's own installation/acceptance reference. Neither is evidence of
recursive benefit by itself.

## Proposed development-method views

The backend and command-line work has advanced beyond the original design:
[identity/execution](contracts/identity-and-execution.md),
[evaluation](contracts/evaluation.md), the
[method registry](contracts/method-registry.md) and
[process selection](contracts/process-selection.md) now have implemented pieces.
The [implementation status](../IMPLEMENTATION_STATUS.md) and
[handoff](../AGENT_HANDOFF.md) describe their exact limits. The executor, evaluator,
call ledger and retained trial records exist separately from this chat adapter.
At this checkpoint, #179/#186 receiver policy/invocation changes remain held.

Possible future read-only projections are:

| Proposed view | Required projection and boundary |
| --- | --- |
| `experiment <id>` | Exact method/dependency/plan identities, execution outcome and known-or-unknown usage. A stored receipt is not independent proof of its measurement origin. |
| `compare <id>` | Frozen arms/slots, scoring outcomes, grants and predeclared criterion. Accumulation, reuse and measured improvement must remain separate claims. |
| Method lineage | Exact published source references, derivation and organization-local decisions; imported source is not accepted or authorized merely because it was found. |
| Adoption outcome / unresolved list | Receiver-bound decisions and reasons, including decline or uncertainty, without converting capability adoption state into a method verdict. |

These are proposals, not implemented commands or new route contracts. Scope any
adapter/API integration through [coordination issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1)
with the current owners. The method/adoption APIs remain separate; chat does not
authorize execution or inherit another organization's authority.

## Rehearsal without ASI:One

With a running local backend and ignored local configuration/token settings:

```sh
uv run --project services/fetch-bridge --frozen python -m genesis_fetch_bridge.acp_mission chat \
  --config .local/acp-mission.json --db .local/acp-mission.sqlite
```

It prints `LOCAL STAND-IN, not ASI:One` and uses the same gateway and journal.
`GENESIS_ACP_BACKEND_URL` and `GENESIS_ACP_AGENT_TOKEN` select one organization.
Use a separate backend, configuration and journal for each organization. Start
with `org`, `methods` and `lineage <installed-capability-id>` where applicable.
A `report <observed development need>` records an observation; a configured
planner must be running to produce a planner decision, and it may choose no
change. This example does not synthesize, execute or transfer a method.

Org A's gateway cannot read Org B's state. Reading installations independently
from each gateway shows their recorded states; it does not establish independent
organizational identities, actual transfer or an accepted A-to-B-to-A loop.
Retained generation/execution results and negative comparisons elsewhere in the
repository are separate evidence. No public routing, fresh comparison result or
goal completion is claimed by this document.
