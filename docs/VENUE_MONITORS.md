# Durable CREAO venue monitoring

An opt-in monitor subscription gives an existing CREAO role continuing work:
assess an explicitly selected verified invocation, then assess later verified
invocations from the same locally accepted capability. Once registered, the
subscription continues through the organization server's workers without a
separate assessment request for each result.

The subscription consumes evidence already produced by capability invocations.
It does not schedule Apify runs, fetch pages, install capabilities, grant new
authority, or choose organizational changes. The existing
[assessment verifier](VENUE_ASSESSMENTS.md) still decides whether a native CREAO
result can create an observation or a replacement requirement.

## Preparation and activation

Use a current CREAO monitor role and its open mission responsibility. Its active
assignment, epoch, provider binding and `monitor.venue` authority must remain
valid. An accountable owner can hold the responsibility without a separate
executor. The exact accepted capability version must be attached to the monitor
role at registration and remain attached while it prepares further work, even
when another role supplies the invocation results. A role that also submits
invocations needs the capability's lookup authority as usual.

Choose an accepted local installation and a recent, independently verified
invocation from that installation in the same mission. Supply explicit event
constraints: selected venue, attendance, budget and currency, requested window,
and whether step-free access is required. These constraints are frozen for the
subscription; later source content cannot alter them.

The initial source is an explicit opt-in. Registration also records the durable
event cursor for future results. Existing older results are not implicitly
replayed. A job created before registration may qualify if its verification
finishes afterward and it still meets the assessment freshness requirements.

Configure every intended invocation input and snapshot in the local trusted suite
before adoption. The [monitoring setup helper](VENUE_PORTAL.md#prepare-continuing-creao-monitoring)
can prepare a baseline and a capacity-cut snapshot for the same event. The suite
and local acceptance evidence remain fixed. Registration cannot authorize an
arbitrary new input or make a changed suite valid.

## Register and inspect a subscription

Use an operator token with `POST /api/v1/venue-monitors`:

```json
{
  "client_request_id": "venue-monitor-1",
  "role_id": "<current-monitor-role>",
  "responsibility_id": "<continuing-responsibility>",
  "source_job_id": "<fresh-verified-invocation-job>",
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
  "expires_at": "<RFC3339 time within the next 24 hours>",
  "max_assessments": 2
}
```

The installation and verification policy are derived from the trusted initial
source. The caller supplies no artifact, acceptance result, oracle answers or
expected verdict. Registration persists the initial assessment intent; it does
not itself make a native request. Save the exact request for retries. Reusing its
key with changed semantics conflicts; exact replay returns historical state and
does not renew authority or expiration.

Poll `GET /api/v1/venue-monitors/{monitor_id}` with the operator token. The record
includes `monitor_id`, `status`, `attention`, `assessment_count`,
`pending_source_job_id`, `pending_assessment_id`, `last_assessment_id` and
`requirement_id`. It also exposes the frozen request, `installation_id`, `suite_id`,
`suite_sha256`, `acceptance_report_ref` and `event_cursor` for correlation.

`status` is `active`, `attention`, `completed` or `stopped`. `assessment_count`
counts prepared intents, including the initial source, rather than successful
outcomes. A pending intent may not yet have an assessment ID. Read saved child
evidence through `GET /api/v1/venue-assessments/{assessment_id}`.

To stop intake, use `POST /api/v1/venue-monitors/{monitor_id}/stop`. Stopping or
expiration prevents new child creation. It does not cancel an already-created
child: exact historical lookup and read-only observation of that child's result
continue. Retained pending IDs must not be interpreted as a new dispatch. There
may be a pending source with no child ID if intake stopped before creation. That
saved intent can remain recorded indefinitely; it will not create a child after
stop or expiration. This differs from an existing child whose native outcome
remains unresolved.

There is no resume endpoint in this version. Preparation failures put the monitor
in `attention` and do not automatically retry child creation or event intake.
Repairing configuration alone does not resume that preparation. The operator
must stop the old subscription and, after resolving any existing child, register
a new explicit subscription. Replaying the old request does not reactivate it.
An already-created child remains observable while in attention: if that same
child later becomes accepted, monitoring can continue when the subscription has
not stopped or expired and its authority and assessment limit still allow work.

## Evidence selection

The worker reads the durable event log in sequence order. A job-success event is
only a discovery signal. Before preparing an assessment, it reconstructs the
private invocation proof and checks its installation, exact artifact, suite,
acceptance evidence, mission and responsibility against the subscription.

Later invocation inputs must equal the initial event input after removing only
the top-level `source_url` field. This permits a separately configured snapshot of
the same event. It cannot silently change attendance, requested times, filters or
other input fields. Matching input alone is insufficient: the result must also
pass its configured suite and the existing venue assessment source checks.

Both source-job creation and verification must be within the assessment's
30-minute preparation window. Later assessments require strictly later-created
source jobs. An older job that finishes late cannot replace an already assessed
newer source. A recent invocation proof does not establish when the underlying
page was fetched; the current source snapshot schema has no fetch timestamp.

## Durable execution and recovery

Before creating a child assessment, the subscription persists its exact request,
including the source job, deterministic request key and deadline. The source
delivery and cursor advance together. Restart recovery reuses that request;
it does not generate a new deadline or a replacement key. Each child deadline is
the earlier of subscription expiration and 30 minutes after intent preparation.
Read-only recovery never extends that deadline. Native work finishing after its
deadline still faces the assessment runtime's deadline checks; observing a saved
child does not authorize late mission effects.

If a process stops after child creation but before recording its ID, replaying
the same request recovers the existing assessment. The existing assessment
runtime owns native dispatch, collection and local verification. The subscription
does not send CREAO requests itself or import a caller-supplied proof.

Only one child assessment may remain unresolved for the responsibility. While it
is queued, running, verifying or unknown, the subscription retains its identifiers
and observes that same child. An unknown native outcome never permits a higher
source to start a new cycle. Temporary local-verification failures remain eligible
for the assessment runtime's bounded recovery; exhausted verification requires
attention under the same saved IDs.

Current mission, responsibility, assignment, lease, epoch, scopes and CREAO binding
are checked before preparing further work. Retirement or reassignment does not
silently transfer the subscription's authority to another role. Normal assessment
checks also apply before provider work and before a result changes the mission.

## Outcomes and bounds

A verified `feasible` assessment records its observation and allows a later
eligible source to be considered. A verified `reopen` assessment records the
existing verifier's replacement requirement. If the subscription is still open,
it becomes `completed` and retains that requirement ID. If it was already stopped
or expired before the worker observes an accepted child, it remains `stopped`
while retaining the child's result and requirement ID.

Reaching the configured assessment limit also produces `completed`, with
`attention: "assessment_limit_reached"`; it does not imply a replacement
requirement. Subscription completion does **not** complete the mission, close the
continuing responsibility, reserve a venue, or satisfy other requirements. The
existing planner can consider any verified replacement requirement.

Native completion or a terminal job status alone is not accepted domain evidence.
Rejected, unresolved or inconsistent results retain attention rather than being
reported as a completed monitoring outcome.

Registration is explicit and bounded: at most 16 open subscriptions per
organization, counting both `active` and `attention`, a requested assessment limit
of 1 through 16, and expiration within 24 hours. `completed` and `stopped` records
do not consume the open-subscription limit. Stopped records with pending intents
remain eligible for read-only child lookup.

Each worker pass visits at most 16 subscriptions, rotating by their last checked
time. For an eligible subscription with no unresolved child, it reads at most 16
events in that pass. The assessment limit includes the initial source. Expiration
and the limit stop new intake; they do not establish that already-created provider
work has stopped.

## Demonstration scope

A subscription can demonstrate continuing organizational responsibility: register
it once, supply a verified baseline, then finish a separately configured updated
invocation. The same native agent should assess both, with an accepted capacity
failure producing one cited replacement requirement. Restart between cycles
should preserve the subscription, agent and completed evidence without replaying
native effects.

Loopback provider tests can establish this control flow and its recovery behavior.
They cannot establish cloud execution, live web freshness, readiness callbacks or
a genuine CREAO Share link. Native sponsor evidence remains separate; see
[the CREAO sponsor notes](CREAO_SPONSOR_NOTES.md).
