# Invention lab: the ASI:One conversation

**Status: command design (ACP Claude session, issue #125). Not implemented beyond
what is marked "exists". No public availability is claimed.** This maps the
product-neutral messages a person would send to a GENESIS organization from an
ASI:One chat onto routes that exist today, and marks everything else as
requiring the method/receipt contract ([#124](https://github.com/HenryWashuHe/genesis/issues/124)),
the evaluator contract ([#126](https://github.com/HenryWashuHe/genesis/issues/126)) or the
adoption contract ([#122](https://github.com/HenryWashuHe/genesis/issues/122),
[adoption.md](adoption.md)). It invents no routes and no wire values.

The implemented adapter is `services/fetch-bridge/genesis_fetch_bridge/acp_mission.py`
(`org`, `report <text>`, `status`; see [FETCH_SPONSOR_NOTES.md](../FETCH_SPONSOR_NOTES.md)).
Everything below keeps its boundary.

## Authority boundary, unchanged

The chat agent holds one credential: the organization's **agent** token. With it
the agent can record evidence and read state and events. It cannot commit a
change, provision a role, publish or install a capability or method, start or
step an acquisition, adoption or experiment, spend budget, or reset an operator
hold. Every message below is therefore either a **read** or **one observation
POST**. A conversation can describe a challenge; the organization's planner and
kernel decide what to do about it. Candidate methods, hidden evaluation cases,
evaluator versions and authority rules are never reachable from chat.

Identity: the signed ACP sender plus message id are journaled before any effect
(open PR #3, was genesis#132; not on main yet); a redelivered message never
repeats an effect, and two senders reusing an id are two reports. Per-sender
cursors keep one judge's `status` separate from another's.

## Messages

Vocabulary is product-neutral: the same words work for any GENESIS organization
under the owner's canonical direction (networked recursive capability
improvement with fixed model weights), including the proposed invention labs;
the venue fixture is only the current regression workload. Each row names the existing route it reads,
or the contract it waits for. "Exists" means on main with tests; "open PR" means
in a reviewed but unmerged PR; "requires contract" means no route or wire value
exists and none is invented here.

| You say | Reply | Reads / effect | Status |
| --- | --- | --- | --- |
| `help` | command list, what chat cannot do | none | exists |
| `org` (also `lab`, `who`) | mission goal, active and retired roles, what each owns, requested capabilities with their real tagged status | `GET /organization` | exists |
| `report <what happened>` (`challenge <…>` is a proposed alias, not implemented) | records one observation with a fixed `source` and `evidence_ref`, then relays this report's own planner decision, proposed and committed changes, roles, provisioning (DEV STUB labeled), recruitment, capability requests and installations, handoffs, retirements; ends with how to follow up | `POST /missions/{active}/observations`, then `GET /events/log` | exists |
| `status` | everything after your last report, paged from a durable per-sender cursor, bounded per reply | `GET /events/log`, `GET /organization` | per-sender paged cursor is in open PR #3 (was genesis#132); on main, status rereads the first page |
| `methods` (also `capabilities`) | installed capabilities and methods by exact `id@version`, their acceptance state and the role holding them; offers one `lineage <id>` button per id | `GET /organization` (`installations`, `roles[].capabilities`) | open PR #5 (ACP commands), not on main; a **method** artifact kind requires #124 |
| `lineage <id>` | the chain for one capability or method: who published which exact version (`ExactContentRef`: domain, schema, id, version, SHA-256), derived from what, who adopted or declined it and why | `GET /capabilities/{id}/versions/{version}` (manifest, provenance, verification); per-candidate decisions `GET /capability-gaps/{id}/adoption` | open PR #5 renders manifest, verification and this organization's installation state; candidate decision log is open PR (genesis#116, here #15); `derived_from` / dependency digests **require #124** |
| `outcome <id>` (also `why`) | an adoption's conclusion: `adopted`, `not_applicable`, `insufficient_evidence`, `no_candidate`, `unknown`, `pending`, `stopped`, with the oracle-free reason | `GET /capability-gaps/{id}/adoption` | open PR #128 (vocabulary from [adoption.md](adoption.md)) |
| `experiment <id>` | one development run: its `DevelopmentRunBinding` (exact method and dependencies as `ExactContentRef`s, public task, evaluator, model settings, purpose), the `ExecutionReceipt` outcome (`output_generated`, failed, interrupted, unknown) and measured-or-unknown usage against the grant | — | names from the merged experimental contracts #18 (`contracts/identity-and-execution.md`); **no route and no executor**; the trusted ledger for calls is Codex's next claim |
| `compare <id>` | a frozen trial plan's arms and slots with each scoring outcome (`Scored`, `Invalid`, `Failed`, `Unknown`) and `EvaluationAccounting`; "improved" never appears unless the predeclared criterion in the plan holds | — | names from merged #22 (`contracts/evaluation.md`) and draft #28 (trial plans); **no route**; the roster starts after outputs exist, so no-output trials come from the pre-execution plan, not from this view |
| `declined` / `unknown` | the most recent declined or unresolved adoptions for this organization, so non-reuse and uncertainty are visible, not hidden | `GET /capability-gaps/{id}/adoption` per gap; a list needs a route | partial: per-gap exists in #128; a list **requires a route** (not proposed here) |

Anything else returns the command list and records nothing.

### Lineage, rendered

When the fields exist, `lineage` renders one line per hop, oldest first, with the
exact identity on each hop and the receiving organization's own decision. The
example is the shape, not data:

```
room-matrix-reader@1.0.0   published by org_a    verified 3/3 on suite portal-a-v1
room-matrix-reader@1.0.0   adopted by org_b      accepted on suite portal-b-v1, 0 synthesis calls
room-matrix-reader@1.1.0   published by org_a    derived from 1.0.0 after failure on layout-drift   ← requires #124 derivation
room-matrix-reader@1.1.0   declined by org_b     not_applicable: suite portal-b-v1 regression       ← requires #128
```

For a development method the same four columns apply: `method v1 → tested by B
→ v2 → accepted by A`. "Improved" never appears in this view; it is a result of
`compare`, and only when its criterion passed.

### What each reply must say about evidence

- Dev-stub bindings say `DEV STUB`. Fixture providers are never described as live.
- A planner decision names its model string as recorded by the kernel.
- `unknown` is said as unknown: "the organization has not confirmed", never
  "no change" and never "failed".
- Publisher claims (scores, applicability) are shown as the publisher's; the
  receiving organization's own acceptance is shown separately.

## Interactive cards (Fetch bonus)

Replies carry an ASI Interactive Card (`DetailCardPayload` in `MetadataContent`,
pinned uagents-core 0.4.12) whose buttons are the next bounded commands: after
`report`, `status` and `org`; after `methods`, one `lineage <id>` per installed
id. A button press arrives as a card response whose `selection.command` must
match the read-only command set exactly (`help`, `org`, `status`, `methods`,
`lineage <id>`); anything else, including `report …`, is ignored. A message
carrying both text and a card response is ambiguous and ignored. Implemented in
open PR #5, not on main; rendering in ASI:One is unverified until routing is.

## Rehearsal without ASI:One

`python -m genesis_fetch_bridge.acp_mission chat --config … --db …` drives the
same gateway, journal and authority boundary from a terminal and prints
"LOCAL STAND-IN, not ASI:One" first. Use one per organization against
`genesis dev` (`GENESIS_ACP_BACKEND_URL`, `GENESIS_ACP_AGENT_TOKEN` for A on
7101 and B on 7102; `acp-mission.example.json` and `acp-mission-org-b.example.json`).
Script: in A `org`, `report the main hall is closed on Nov 14` (the planner
must be running with a model for the reply to go beyond the recorded evidence;
say so otherwise), `methods`, `lineage <id>`; in B `methods` to show the adopted
exact version. Never present the stand-in as ASI:One routing.

## Two organizations in one conversation

Each organization runs its own gateway with its own seed, journal and agent
token (`acp-mission.example.json` per organization). A judge asks Org A
`report …`, then asks Org B `org` and sees the adopted version. The
conversation never bridges the two: Org A's gateway cannot read Org B's state.
Inheritance is visible because each organization reports its own installations.

## Not claimed

- No Agentverse registration, Inspector/mailbox connection or ASI:One routing
  has been performed from this repository; those need an account login and are
  recorded separately when done.
- No method has been developed, adopted, executed or shown to help.
- No route is added by this document. `experiment`, `compare` and a `declined`
  list wait for #124/#126 contracts and a scoped route claim in issue #4.

## Reconciliation with the merged contracts

#18 (`contracts/identity-and-execution.md`) and #22 (`contracts/evaluation.md`)
are merged as experimental typed records: `ExactContentRef`, `DevelopmentRunBinding`,
`ExecutionReceipt` with purposes `ordinary` / `acceptance` / `comparison` and a
per-dimension grant, `EvaluationRequest` and scoring outcomes `Scored` / `Invalid`
/ `Failed` / `Unknown`, `EvaluationAccounting`, and the request roster. The rows
above use those names. None of them has a route, an executor, an evaluator or a
trial-plan runtime yet (draft #28 freezes plans; the executor lane is Henry's
Claude's `experiments/invention-lab/executor/**`; the call ledger is Codex's next
claim). A chat view of them is read-only projection of records the kernel will
hold; it is not proposed here as a route.
