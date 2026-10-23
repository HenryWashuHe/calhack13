# Persistent CREAO venue assessment

A continuing CREAO specialist can turn an independently verified venue lookup
into mission work. Each update freezes event constraints, the selected venue's
facts and citations, the invocation proof, and the specialist's current assignment.
CREAO proposes a structured assessment. GENESIS independently checks every
predicate before recording an observation or creating replacement work.

This endpoint supports operator-triggered updates with automatic dispatch,
collection and local finalization. For continuing work after the client exits,
register a [bounded monitor subscription](VENUE_MONITORS.md) once: it consumes
later eligible verified invocation events and prepares assessments through this
same path. Neither interface subscribes to arbitrary web changes or replaces the
planner. The same role and native agent can handle successive updates with fresh
conversations. Persistence means continuing ownership and durable execution
history; conversation-memory reuse is not asserted.

## Prepare a monitor

The role must be active, bound to CREAO, and own the active responsibility for the
mission. Grant `monitor.venue` through a normal organization change; keep
`capability.venue_lookup` if the role also executes its installed Actor. Configure
the existing CREAO specialist runtime and its private journal normally. A dev stub
or operator-supplied result cannot substitute for a native execution receipt.

An eligible source is a completed, **semantically verified invocation** in the
same organization and mission. Acceptance test runs, arbitrary JSON, foreign
jobs, unsupported extraction, and missing selected venues are refused. Both the
source job's creation and its verification must be within the last 30 minutes at
preparation. A recent verified invocation does not independently prove the web
page itself was freshly fetched: the current source schema has no fetch time.
The configured suite and snapshot policy still define what evidence is supported.

## Submit an update

Use an operator token with `POST /api/v1/venue-assessments`:

```json
{
  "client_request_id": "venue-update-1",
  "role_id": "<current-monitor-role>",
  "responsibility_id": "<continuing-responsibility>",
  "source_job_id": "<verified-invocation-job>",
  "policy": {
    "selected_venue_id": "grand-hall",
    "attendees": 120,
    "budget": {"minor_units": 80000, "currency": "USD"},
    "window": {
      "start": "2026-11-14T18:00:00-08:00",
      "end": "2026-11-14T22:00:00-08:00"
    },
    "step_free_required": true
  },
  "deadline": "<RFC3339 time within the next 24 hours>"
}
```

The deadline bounds the assessment execution, not the event date. Save the exact
request including its deadline for retries. Repeating its key returns the same
assessment and job. Changing any field under that key conflicts. A source job can
be used once per responsibility; later updates must reference later-created
invocations. Only one unresolved assessment may exist per responsibility. An
unknown native submission blocks a new cycle until reconciled; it never silently
creates another run.

The response includes `assessment_id`, `job_id`, `source_job_id`, `status`, `proof`,
`finalization_attempts`, and `attention`. Poll
`GET /api/v1/venue-assessments/{assessment_id}` with the operator token. The normal
server worker finalizes collected output automatically. The operator can retry
local verification with `POST /api/v1/venue-assessments/{assessment_id}/verify`;
this endpoint performs no provider HTTP request.

## Native output and mission effects

The job input contains `context` and a strict `response_schema`. Context includes
only verified venue facts, explicit constraints and their bound identifiers; it
never includes the oracle's expected answer. The Developer API's completed result
must contain a `text` string with exact JSON matching that schema (at most 16 KiB).
Markdown fences, duplicate keys, unknown fields and fabricated citations fail.

GENESIS recomputes capacity, price with exact currency, containment of the event
window using parsed timestamps, declared availability, and the declared step-free
attribute. Every check and citation must match. `feasible` requires all five
checks; otherwise the valid assessment must say `reopen`.

Native completion first leaves the job `VERIFYING` with `domain_verified: false`.
The domain finalizer reconstructs the execution receipt from CREAO's durable
journal and frozen job/agent binding. It never trusts copied `job.result`, generic
result submissions, or the provider's completion flag as mission success.

An accepted assessment atomically records an observation and finishes its job.
A verified `reopen` additionally creates one `UNMET` replacement requirement owned
by the mission controller, citing that observation. The existing planner can
consider the new requirement. The mission and continuing monitor responsibility
remain open. A feasible assessment neither satisfies other requirements nor makes
a reservation. Repeated verification creates no duplicate mission effects.

The work-in-progress [Fetch venue recovery API](VENUE_RECOVERY.md) lets an operator
use an accepted Reopen plus verified same-event candidate invocations to recruit
a replacement selector and prepare one follow-up assessment. It uses a separate
recovery responsibility, preserving the continuing monitor's assignment.

The final proof preserves the native run and agent IDs, receipt digest, invocation
proof reference, accepted structured output, decision revision and observation /
requirement IDs. Historical reads reconstruct the decision against saved authority
and the private native receipt. Keep the original provider journal and configured
suite available; a missing or changed evidence chain fails closed.

## Authority, interruption and evidence limits

The frozen `monitor.venue` scope, assignment/epoch, active mission, lease/deadline
and full CREAO binding are checked at preparation, before provider work, after
runtime and account queue admission, and before applying a result. The kernel lock
is released before HTTP. Authority can still change during the unavoidable
network interval; post-result checks prevent those results changing the mission.

Guarded calls persist their effect claim only after queue admission and the final
local authority check, before sending HTTP. Cancellation while waiting leaves a
safe prepared/waiting operation. Cancellation after the claim or an ambiguous
POST remains an unknown outcome and is never automatically replayed. The existing
runtime owns retries; the assessment finalizer performs only local reads/writes.
If a queued assessment restarts without CREAO configured, its outbox delivery
remains pending with an adapter error; the development stub cannot acknowledge
it. Restoring configuration lets that same dispatch resume under its original
deadline and the existing bounded outbox retry policy.

Local finalization is limited to 16 records per pass and three automatic attempts.
A storage or evidence error retains attention and the same collected run for
operator repair/retry. A crash during a claim retains `local_verification_pending`.
There is one organization process per database; the kernel mutex is not a
cross-process lease.

The integration fixture proves two same-event source snapshots (baseline and
reduced capacity) through real Rust Apify/CREAO clients against loopback servers,
including restart and no additional agent creation. Initial publication and the
already-bound monitor are explicitly synthetic trusted setup. This is neither a
live sponsor run nor evidence of native agent creation/readiness.

For CREAO eligibility, separately capture the actual agent's build/save provenance,
real execution and a genuine Share link. The Developer API references inspected
here expose native IDs and result text, not a share-link field; do not manufacture
a share URL. See [the sponsor checklist](CREAO_SPONSOR_NOTES.md),
[async run creation](https://docs.creao.ai/developer-api/runs/create-async-run),
[run status](https://docs.creao.ai/developer-api/runs/get-run-status), and
[CREAO's Share description](https://docs.creao.ai/trust-and-safety/privacy).
