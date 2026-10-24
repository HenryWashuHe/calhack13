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

Identity: the signed ACP sender and message id are journaled before any effect
(PR [#132](https://github.com/HenryWashuHe/genesis/pull/132)); a redelivered
message never repeats an effect. Per-sender cursors keep one judge's `status`
separate from another's.

## Messages

Vocabulary is product-neutral: the same words work for a venue organization
today and an invention lab later. Each row names the existing route it reads,
or the contract it waits for. "Exists" means on main with tests; "open PR" means
in a reviewed but unmerged PR; "requires contract" means no route or wire value
exists and none is invented here.

| You say | Reply | Reads / effect | Status |
| --- | --- | --- | --- |
| `help` | command list, what chat cannot do | none | exists |
| `org` (also `lab`, `who`) | mission goal, active and retired roles, what each owns, requested capabilities with their real tagged status | `GET /organization` | exists |
| `report <what happened>` (also `challenge <…>`) | records one observation with a fixed `source` and `evidence_ref`, then relays this report's own planner decision, proposed and committed changes, roles, provisioning (DEV STUB labeled), recruitment, capability requests and installations, handoffs, retirements; ends with how to follow up | `POST /missions/{active}/observations`, then `GET /events/log` | exists |
| `status` | everything after your last report, paged from a durable per-sender cursor, bounded per reply | `GET /events/log`, `GET /organization` | exists (#132) |
| `methods` (also `capabilities`) | installed capabilities and methods by exact `id@version`, their acceptance state and the role holding them | `GET /organization` (`installations`, `roles[].capabilities`) | exists for task capabilities; a **method** artifact kind requires #124 |
| `lineage <id>` | the chain for one capability or method: who published which version, derived from what, who adopted or declined it and why | `GET /capabilities/{id}/versions/{version}` (manifest, provenance, verification); per-candidate decisions `GET /capability-gaps/{id}/adoption` | manifest and provenance exist; candidate decision log is open PR #116; `derived_from` / dependency digests **require #124** |
| `outcome <id>` (also `why`) | an adoption's conclusion: `adopted`, `not_applicable`, `insufficient_evidence`, `no_candidate`, `unknown`, `pending`, `stopped`, with the oracle-free reason | `GET /capability-gaps/{id}/adoption` | open PR #128 (vocabulary from [adoption.md](adoption.md)) |
| `experiment <id>` | one experiment's task, method version, consumed budget, outcome and evidence reference | — | **requires contract** (#124 receipt, #126 evaluator); no route |
| `compare <id>` | the declared comparison for a return challenge: treatments, budgets, scores, "improved" only when the predeclared criterion holds | — | **requires contract** (#124 comparison protocol); no route |
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

## Interactive cards (optional, Fetch bonus)

ASI Interactive Cards can offer the next bounded message as a button: after
`report`, buttons for `status` and `org`; after `methods`, one `lineage <id>`
per installed id. Buttons send exactly these text commands. No card may expose
an operator action. This is a presentation choice and is not implemented.

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

## Reconciliation with #124 and #126

When Codex's contract lands under `docs/invention-lab/contracts/**`, this
document is updated to its names (artifact kind, identity and digest fields,
receipt fields, comparison treatments) in a docs-only PR, and the change is
posted in #125. Until then every field above that is not on main is labeled
"requires contract".
