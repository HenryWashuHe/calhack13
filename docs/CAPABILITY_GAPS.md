# Capability gaps

A capability gap is the organization's committed, evidence-bound statement
that a role needs an executable capability it does not have. It closes the
loop between the planner and capability acquisition without giving the
proposing agent any say over how a capability is built, tested or approved.

```
observation (evidence_ref) ─► requirement ─► planner proposal
   AssignResponsibility(owner = role) + RequestCapability(role, responsibility)
                                   │  kernel validates and commits
                                   ▼
                     CapabilityGap { status: open }
                                   │  acquisition driver (operator policy)
                                   ▼
          acquiring ─► acquired { capability@version } │ refused { reason }
                                   │  installed { installation_id } │ refused
                                   │  + one kernel observation, same commit
                                   ▼
                  planner's next pass (the runner plans on that observation)
                                   ▼
                  AttachCapability(role, capability@version)
```

### Closing the loop

The planner only plans on new observations, so a final transition records
one itself. `installed` and `refused` each add an observation on the gap's
mission (source `kernel.capability_gap`, `evidence_ref`
`genesis://capability-gaps/<gap_id>`) in the same outcome as the status
change, so the note can't be lost or duplicated. A repeated transition is a
no-op and adds nothing. The note names the role, requirement and, when
installed, the exact version and installation. The planner then attaches it,
or decides what to do without it after a refusal. The note has no public
URL, so it can't justify another `RequestCapability` on its own (the origin
check rejects it). The requirement's evidence still could, so repeated
refusals of the same request are bounded below.
`Acquiring` and `Acquired` record nothing: acquired is not installed. The
note is recorded only while the gap's mission is `Active` and before its
deadline. A gap of a finished, failed or expired mission still records its
final status (the `capability_gap_status_changed` event is the audit) but
gives the planner no new input, so a late refusal can't revive closed work.

## The operation

```json
{"op": "RequestCapability", "key": "reader", "requirement_id": "$REQUIREMENT",
 "role": {"kind": "existing", "role_id": "role_…"},
 "responsibility": {"kind": "new", "key": "read"},
 "capability_id": "listing-reader",
 "purpose": "Extract options from the listing site",
 "observed_sources": ["https://portal.example/listings/table.html"]}
```

The kernel commits it only if:

- the requirement exists, is not satisfied, and belongs to the
  responsibility's mission;
- the role resolves (existing ACTIVE, or created earlier in the change) and
  **owns** the responsibility when the operation applies (assign it first);
- the responsibility was created **for this exact requirement**
  (`responsibility.requirement_id == requirement_id`) and the requirement is
  owned by the same role, the link acquisition authority checks later;
- `capability_id` is 3–64 characters of `a-z 0-9 - .`, starting alphanumeric;
  `purpose` is 1–400 characters;
- `observed_sources` holds 1–8 URLs the forge can use: **canonical https**, a
  host, no credentials, no fragment, at most 1024 bytes (the same rules as
  `SynthesisRequest::validate`), each on the **same origin** (scheme and host,
  case-insensitive) as an `evidence_ref` of the requirement's triggering
  observations. The agent cannot invent targets, and a committed gap is always
  one a trusted policy could materialize. So the capability path needs the
  evidence itself served over https (the portal's public tunnel); a local
  `http://` portal cannot back a request;
- no installed, approved capability already has that ID (use
  `AttachCapability`);
- no live gap exists for the same requirement and capability ID, and that
  pair has been refused fewer than 2 times
  (`MAX_REFUSED_REQUESTS_PER_REQUIREMENT`; `capability_gap_limit_exceeded`).
  A refusal wakes the planner, and the requirement's own evidence still
  satisfies the origin check, so without this a planner that keeps asking
  would loop forever at one model call and two commits per cycle. Changed
  circumstances come in as new evidence, so a new requirement starts fresh;
- the organization holds fewer than 16 live (non-terminal) gaps, and the
  requirement fewer than 3 (`MAX_LIVE_CAPABILITY_GAPS`,
  `MAX_LIVE_GAPS_PER_REQUIREMENT`; `capability_gap_limit_exceeded`). Each gap
  can become an acquisition, so an agent must not be able to flood the driver.

Commit adds `OrgState.capability_gaps[gap_id]` and emits
`capability_gap_opened`. The operation carries no suite, oracle, quota,
provider or candidate naming.

## Transitions

Verified acquisition, registry publication and local installation are
distinct facts. Only `installed` (an accepted local installation) makes the
capability attachable; `AttachCapability` still checks the installation.

`engine::update_capability_gap` allows only:

| From | To | Payload / checks |
| --- | --- | --- |
| `open` | `acquiring` | `acquisition_id` (1–400) |
| `open` | `refused` | `reason` (1–400), no acquisition |
| `acquiring` | `acquired` | same `acquisition_id`; this gap's `capability_id` at an exact version (not `latest`); optional `publication_ref` |
| `acquired` (no `publication_ref`) | `acquired` (with `publication_ref`) | publication is a separate effect: record its reference **once**, same acquisition and capability; never replace or remove it |
| `acquiring` | `refused` | `reason`, same `acquisition_id` |
| `acquired` | `installed` | same acquisition and capability; `installation_id` must exist here for that exact capability with status **Accepted** |
| `acquired` | `refused` | `reason`, same `acquisition_id` |

`installed` and `refused` are final. A refusal keeps the bound
`acquisition_id`, so the failed attempt stays on record; genuinely new work
needs a new gap. Each transition increments the revision and emits
`capability_gap_status_changed {from, to}`. Transitions are backend-only:
there is no org-change operation or HTTP route that writes them.

The gap also freezes, at commit, the requirement's triggering observation IDs
and `requirement_digest` (canonical SHA-256 of requirement ID, mission,
summary, acceptance and evidence), so a driver can confirm it acts on the
same need.

## Driver boundary (genesis-server)

`genesis_server::capability_gaps`:

- `open_gaps(&app)` lists unclaimed gaps, oldest revision first.
- `transition(&app, &gap_id, status)` applies one transition atomically with
  its event. Repeating an already-applied transition returns the gap with no
  second event, so a driver can retry after a lost response.

An acquisition driver is expected to: take an open gap, look up **trusted
operator policy** for its `capability_id` (suite ID, quotas, candidate/repair
naming, allowed source URLs), intersect `observed_sources` with the policy
allowlist, create exactly one acquisition intent with the gap ID as its
idempotency key, then move the gap to `acquiring`. On a published,
independently verified result it moves the gap to `acquired` (with a
publication reference if published); once an accepted local installation
exists, to `installed`; on refusal,
quota exhaustion or rejection it moves to `refused` with a reason. Live
authority (mission, assignment lease, role state) is checked by the
acquisition runtime as it is today.

The driver itself is not part of this change.

## Planner

The planner's system prompt lists `RequestCapability` as option (c), after
assigning existing roles and attaching installed capabilities, and states
that it never chooses how a capability is built or tested. Its state message
lists requested capabilities with their status. It attaches a requested
capability only once it appears under installed capabilities, and does not
request a capability that is already requested and not refused.

## Tests

- `genesis-core/tests/engine_fuzz.rs`: 3,000 randomized proposals from a
  fixed seed, chained through successful commits, with random gap
  transitions. The engine never panics and the live-gap limit always holds.

- `genesis-core/tests/capability_gap.rs`: happy path, origin binding (other
  origins, non-http, credentials, empty, non-URL evidence, case-insensitive
  match), ownership, name/purpose rules, already-installed capability,
  duplicate live gap, forward-only transitions, and deserializing older
  records without the new fields.
- `genesis-server/tests/capability_gaps.rs`: a gap committed through
  `POST /org-changes` is listed, transitions are idempotent, invalid moves are
  refused, and the event log has exactly one event per real transition.
- `genesis-planner/tests/planner.rs`: a scripted model's `RequestCapability`
  commits through the real kernel as an open gap.
