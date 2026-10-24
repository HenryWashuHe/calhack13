# Automatic venue recovery intake

Set `GENESIS_AUTOMATIC_VENUE_RECOVERY=true` on an organization server to let an
accepted CREAO `Reopen` decision initiate its existing Fetch recovery coordinator.
The default is off. Configuring a Fetch bridge alone does not enable recruitment.
This opt-in also considers historical accepted decisions whose source evidence
remains fresh. Each organization has its own SQLite cursor and intake journal.

The organization must already have the monitoring role, verified invocation
evidence, the private Fetch runtime and an independently configured selector
peer. Current role/mission/grant/quota checks must allow the proposed new
responsibility and contractor. The same core checks run before saving a request
and again before committing the organization change.

## What becomes automatic

1. Persist a completion-event hint before reconstructing the assessment's private
   CREAO receipt. Only a backend-accepted `Reopen` can create new recovery work.
2. Discover fresh succeeded Apify invocation jobs for the same event, including
   sources verified before the Reopen. Event envelope mission IDs are not needed;
   the worker checks each actual job and authenticated proof.
3. Choose the newest authenticated source by source creation time, then job ID.
   This slice uses one source containing up to 16 candidate venues. The complete
   captured backlog is examined before selection. An independently checked
   feasible alternative must exist under the original policy. The preflight
   reveals no expected winner to the Fetch specialist.
4. Save the exact request key, source binding, candidate job and deadline before
   creating the recovery. A subsequent pass recruits the independent Fetch
   specialist and delegates a new responsibility through the existing kernel
   checks. No per-recovery operator POST is required.
5. The existing coordinator authenticates Fetch task receipts, verifies the
   selection, obtains an independent CREAO assessment and retires its contract.
   Intake's `linked` state means the coordinator exists; read that coordinator
   for the actual outcome and remote release status.

There is no new page-fetching or Actor invocation in this worker. If an
alternative is absent, the intake waits for another independently verified
invocation. It does not broaden the frozen event policy or fabricate evidence.
Capability creation, publication and independent organizational inheritance
remain separate parts of the wider GENESIS mission.

## Identity, uncertainty and limits

Source matching reuses recovery validation: organization, mission, installation,
full oracle suite digest, acceptance, artifact pin and exact canonical event
input, removing only `source_url`. Different responsibilities may supply the
same event's evidence. Venue citations must match the verified snapshot. A
feasible copy of the failed venue does not count as a replacement.

Unreadable same-event proof evidence holds preparation, including a missing
verification row. The worker retries a bounded scan and retains attention rather
than silently choosing an older source. Authenticated unrelated or unsupported
evidence is skipped. No feasible alternative creates no request or contract.

The deadline is capped by original and candidate creation/verification times
plus 30 minutes, and by the remaining intake lifetime. Future timestamps are
refused. Microsecond normalization rounds down. After preparation neither a
newer source nor a restart can replace the request or renew its deadline.

Exact-request reconciliation runs before freshness/configuration checks. An
operator-created recovery for the same accepted Reopen is linked as
`externally_managed`; intake does not claim to have created it. An existing
historical coordinator can still be linked when providers are unavailable or
the source has expired. A commit followed by a lost intake-link write recovers
the original coordinator. The database's existing unique requirement constraint
also protects against a second recovery.

Disabling the flag stops discovery, preparation and new recovery creation.
Already-saved rows can still reconcile an existing coordinator. Existing recovery
workers retain their own completion/retirement behavior. Enabling again resumes
the original deadlines; it grants no extra freshness.

Each 500 ms pass inspects at most 64 relevant assessment completion events and
16 active intake rows, ordered by last inspection. Each row scans at most 16
relevant fresh Apify completion events. Unrelated historical SSE events do not
consume these pages. Missing proof evidence and corrupt rows cannot monopolize
the whole worker page. There is a 256-row lifetime intake limit, including
ignored/expired decisions, and the existing separate 256-recovery limit. A full
journal reports attention and preserves the discovery cursor for the unhandled
event; existing rows continue to advance. Very large relevant backlogs can still
consume the freshness window. This is a bounded single-process demo worker, not
a cross-process lease or unlimited scheduler.

## Inspection and evidence

`GET /api/v1/venue-recovery-intakes` accepts the existing operator or agent token.
It returns assessment ID, phase, attention, request/candidate/deadline once
prepared, linked recovery ID and whether that recovery was externally managed.
Raw candidate facts and native provider messages stay out of this projection.

Phases: `observing`, `discovering`, `prepared`, `linked`, `ignored`, `expired`.
`awaiting_feasible_verified_source` means new evidence is needed;
`awaiting_source_verification` preserves uncertainty in discovered evidence;
`awaiting_recovery_providers` means the required runtimes are not configured.
Other verification/authority failures retain the row with attention for later
inspection. Expired work does not start a new request automatically.

The integration suite covers genuine local invocation/assessment receipts,
durable preparation, restarts, duplicate drains, manual recovery, lost-link
reconciliation, waiting for a later feasible source, irrelevant event backlog,
and intake through Fetch selection, CREAO acceptance and contract retirement.
Providers and transport responses are loopback fixtures. This does not establish
Agentverse registration, ASI:One routing, cloud execution, a venue booking,
requirement closure, or completion of the wider autonomous mission.
