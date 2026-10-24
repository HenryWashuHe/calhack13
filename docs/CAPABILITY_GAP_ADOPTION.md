# Adopt before acquiring (`open` → `installed` by adoption)

When an organization commits `RequestCapability`, the gap driver
([CAPABILITY_GAP_POLICY.md](CAPABILITY_GAP_POLICY.md)) synthesizes a new
capability. If another organization has already published something that
serves the need, that is wasted model and Actor spend. Adopt-first makes the
gap try independent adoption first: B searches the registry, installs a
foreign publication and accepts it only with its own frozen suite. Synthesis
runs only after a definitive empty search, and only if the rule allows it.

Code: `crates/genesis-server/src/capability_gap_adoption.rs`.
Kernel transition: `engine::install_capability_gap_by_adoption_at`.
Tests: `crates/genesis-server/tests/gap_adoption.rs`,
`crates/genesis-core/tests/capability_gap.rs`.

## Turning it on

Off by default. Add to the `genesis org` command:

```bash
genesis org ... --registry-url https://registry.example --registry-token ... \
  --gap-adoption-policy adopt.json
```

It needs a capability registry and a real Apify executor (B's checks run the
adopted build). `adopt.json` is trusted operator configuration:

```json
{"rules": [{
  "capability_id": "venue-reader",
  "suite_id": "independent-venue-suite-b",
  "synthesis_fallback": true
}]}
```

`capability_id` is the capability the gap requests. The adopted capability may
have a different ID; B's suite decides whether it serves the need.
`synthesis_fallback` lets a definitive empty search hand the gap to the gap
driver (which must be configured separately). Without it the gap stops at
attention. The empty result belongs to the frozen grant: if the assignment or
epoch has changed since (even for the same role), the gap is refused on
authority instead, and nothing is handed to synthesis.

The planner learns these names from `GET /api/v1/capability-gaps/requestable`,
where a name with no acquisition rule is marked `adoption_only` (see
[CAPABILITY_GAP_POLICY.md](CAPABILITY_GAP_POLICY.md)). A request under any
other name is never adopted.

## What it does

For each open gap with a rule, one stage per pass (the worker runs twice a
second):

1. **Freeze** one record: requirement digest, the requesting role's current
   assignment and epoch, the rule's suite and its digest, and the fallback
   choice. From now on the gap driver skips this gap.
2. **Dispatch** one adoption with key `gap-adopt:<gap>` after rechecking the
   live requirement (still assigned, same digest), the grant and the suite.
   The adoption store is idempotent by key, so a crash after creation
   recovers the same adoption. Its ID is recorded before anything else.
3. The existing adoption worker discovers (excluding this organization's own
   publications), installs the selection and runs B's frozen suite through
   the capability worker. Nothing in this module calls a provider.
4. **Link**: in the adoption's acceptance transaction, if this record still
   owns the adoption and its authority is current, the gap is recorded
   `installed` with the exact selected pin and accepted installation, and the
   record is closed. The kernel's final gap note wakes the planner to attach
   the capability.

Status (operator token): `GET /api/v1/capability-gaps/{id}/adoption` returns
state (`active`, `released`, `installed`, `attention`), an attention code, the
adoption ID, the fallback choice, the selected capability and the
installation ID.

## Outcomes

| Adoption result | Gap adoption | Kernel gap | Gap driver |
| --- | --- | --- | --- |
| Accepted (authority current) | `installed` | `installed`, same transaction as acceptance | never runs |
| `no_candidate`, fallback allowed | `released` (terminal, once) | stays `open` | binds and synthesizes |
| `no_candidate`, no fallback | `refused: no_candidate` | `refused` (planner note) | never runs |
| B's suite rejected the selection | `refused: rejected` | `refused` | never runs |
| Fenced, or requirement/assignment changed | `refused: authority` | `refused` | never runs |
| Registry unavailable or ambiguous | `attention: registry` | `open` | skips |
| Invalid candidate | `attention: candidate` | `open` | skips |
| A check failed or its outcome is unknown | `attention: checks` | `open` | skips |
| Installation conflict (e.g. a sibling gap already installed the same pin) | `attention: conflict` | `open` | skips |
| Key already bound to a different intent | `attention: invalid_record` | `open` | skips |
| Another adoption holds the requirement grant, or the store is at capacity | stays `active` with code `conflict`; dispatches when the slot frees | `open` | skips |

Definite outcomes refuse the gap the way the gap driver does, so the planner
hears about it and the gap stops counting against the live-gap limits; the
refusal carries the adoption ID. A requirement that changed before anything
was frozen is refused too (no record is written). A role still waiting for
activation is not a refusal: the gap waits, deferred from the driver.

Uncertain outcomes stop at attention with the adoption ID kept and never fall
back to synthesis: an unknown result is not an empty result. The gap stays
`open`.

## Retrying a stop

`POST /api/v1/capability-gaps/{id}/adoption/retry` (operator) starts one fresh
adoption for a gap stopped at `registry`, `candidate`, `configuration`,
`conflict` or `checks`, while the gap is still open: the stopped adoption moves to
`previous_adoption_ids`, the record returns to `active` with `attempt` + 1,
and the coordinator dispatches under the key `gap-adopt:<gap>:<attempt>`
after rechecking the requirement and grant. At most two retries per gap.
Whatever the code, a retry also needs every native check run of the stopped
adoption to carry terminal provider evidence or never to have been accepted
by the provider: `collected`; `failed` with no receipt (the submission never
reached the provider, or its response already reported a terminal failure);
`failed` with a receipt whose failure is `RunFailed` (a status read of that
exact run and build reported Apify `FAILED`, `TIMED-OUT` or `ABORTED`); or
`prepared` under an adoption that left checking. Anything else answers 409
`adoption_checks_unresolved` and leaves the record untouched: an unknown
outcome (`needs_reconciliation`, e.g. a restart mid-POST), a run in flight,
`rejected` (a local verdict that can overwrite a submitted or still-running
run without polling it, including one that also lost its authority), and any
other `failed` with a receipt (an HTTP status-read failure such as 401, or
output evidence that didn't match the saved request: neither says the
provider run ended). `RunFailed` is a distinct provider error that the
capability runtime keeps through sanitization; `Rejected` is not evidence of
termination. There is no reconciliation of the blocked runs yet, so the gap
stays at attention. In practice retry recovers registry, candidate and
conflict stops (no checks yet) and checks with collected evidence or a
confirmed terminal failure (before submission, in the submission response, or
from a later read of the saved ticket); it does not recover a check whose
status read failed, or a stop that interrupted one. Automatic paths never
retry. `invalid_record` is never retryable. A retry never releases the gap to
synthesis; only a definitive empty search does. Status shows `attempt`,
`previous_adoption_ids` and whether a retry is allowed now (`retryable`).

## Ownership

The gap driver defers a gap while adoption owns it: the gap's capability has a
rule and the driver has not already bound it, or a gap-adoption record exists
that is not `released`. Unreadable records defer. If the driver had already
bound a gap before adopt-first was enabled, the driver keeps it.

A frozen record finishes under its frozen choices even if the policy is later
removed: the coordinator always runs, so a removed policy stops new freezes,
not existing intents. The acceptance link is found by adoption ID, or, if the
process stopped between creating the adoption and saving its ID, by the
coordinator's exact key and binding. A storage error while linking fails the
acceptance transaction so it is retried whole, never split.

## Why a separate kernel transition

Ordinary `update_capability_gap_at` requires the installed capability to match
the gap's requested ID exactly. Adoption may select a differently named
capability, so `install_capability_gap_by_adoption_at` is a separate,
backend-only transition: the gap must be open, the installation accepted with
exactly the given capability and exact version (`latest` is refused), and the
adoption ID becomes the gap's link. A caller cannot reach it by choosing an
`adoption:`-style ID for the ordinary transition (tested). No HTTP route calls
it; only the adoption acceptance hook does, after checking that this record's
key, requirement, grant and suite own that adoption.

## Evidence

`tests/gap_adoption.rs`, real kernel commits and SQLite, a real registry with
org A's and org B's publications, real Rust Apify and model clients against a
loopback provider:

- `serve_org` alone takes a committed gap to `installed` with A's
  differently named capability: one adoption, one search, one run per B case,
  zero model calls, zero Actor creations, no gap acquisition, one planner note;
  a restart changes nothing; the history audit is clean;
- concurrent passes and reopening between stages keep one record and one
  adoption;
- an empty search (only B's own publication) releases the gap once; the
  driver then binds exactly one acquisition; further passes change nothing;
- without fallback the same search stops at attention;
- an unreachable registry and an ambiguous check run stop at attention with
  the adoption kept, nothing resubmitted and no synthesis;
- a changed requirement before dispatch creates no adoption; during checks
  it prevents both acceptance linkage and synthesis;
- an operator's ordinary accepted adoption for the same requirement leaves
  the gap open; a squatted `gap-adopt:` key is refused as an invalid record;
- a requirement changed before freeze is refused exactly as without
  adopt-first (one planner note); removing the policy after dispatch still
  releases and hands over once; a second ruled gap on the same requirement
  waits for the slot, then reports its own installation conflict; an
  acceptance that lands before the adoption ID was saved still links.

`tests/two_org_inheritance.rs` runs both organizations end to end with real
`serve_org` and no operator calls: A's gap is synthesized, verified,
published and installed (#66, #78); B's later gap for the same need, under a
different capability name, installs A's exact version after B's own suite
accepts it. Provider calls are attributed per organization by key: B makes
zero model calls and zero Actor creations, and runs only its own suite
inputs.

Timing matters: if B's need is committed before A publishes, B's search is
empty. With `synthesis_fallback` B then synthesizes its own; without it B's
gap is refused (`no_candidate`) and the planner may request again later.

No live provider was contacted.
