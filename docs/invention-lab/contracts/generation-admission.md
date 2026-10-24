# Admission of declared generation attempts

`GenerationAdmission` binds a validated `GenerationRoster` and a list of exact
Prepared checkpoint identities permanently reserved under it. `reserve(session)`
requires an exact declared, still Prepared, never-reserved session. Its roster
already bounds the number of requests and the sum of requested maximum output
tokens. Reserving fewer attempts cannot exceed those caps. Reservations never
refund after failure, rejection, unknown outcome or unused capacity.

`is_reserved(session)` validates the entire declared identity, including the
saved historical request and initial checkpoint hash, at any phase. A terminal
session without a reservation has no retroactive admission. `summary()` binds
the current journal and roster digests, reserved identities, count and output
caps. These caps are not observed usage; actual and unknown costs still belong
in the existing roster/cost assessments.
The [admission-aware cost assessment](admitted-costs.md) binds this journal and
its full original roster to later observations and one optional trial, without
reserving, refunding or rewriting evidence.

This API performs no IO. An integrating host must exclusively lock the journal
and session, persist the reservation durably, then persist Requesting intent
before using the existing one-attempt provider path. A crash between writes can
strand a reservation even while the session remains Prepared. Refuse another
request for that identity; do not refund or reconstruct an unused slot. Keep
the journal alongside every experiment's original roster and generation files.

These are bounds for participating commands using one trusted journal, not an
account-wide quota, billing guarantee, input-token or aggregate wall limit.
Provider caps/behavior and trusted operator storage remain separate boundaries.
Copies, rollback, alternate ungated commands and a different journal can bypass
cooperative admission; hashes do not authenticate a writer or establish complete
history or chronology. Foundation weights and provider-default sampling are
not frozen by this mechanism. It grants no execution/adoption authority and
does not establish a completed comparison or recursive benefit.

## CLI

Create one journal from the original saved roster before stepping its attempts:

```sh
cargo run -p genesis-forge -- method-admission-prepare \
  --roster roster.json --admission admission.json
cargo run -p genesis-forge -- method-generate-step \
  --checkpoint generation.json --admission admission.json
cargo run -p genesis-forge -- method-admission-status --admission admission.json
```

Preparation is offline and refuses overwrite. Status is offline/read-only and
does not acquire locks or reconcile state. The step holds the admission lock
before the generation lock until the final session save completes. Exact
membership and reservation status are checked before credentials; invalid model
configuration consumes no reservation. After preflight, it durably saves the
reservation before Requesting and the existing single provider call.

A stranded Prepared reservation returns an error without a provider request.
A reserved Requesting session uses existing reconciliation, with no replay or
refund. An advanced session lacking a reservation is refused. Failed, rejected
and uncertain attempts retain the full reserved caps. Actual reported usage is
still assessed separately; a reservation alone is not proof a request was sent.

Use one retained canonical journal pathname for every participating step. Locks
are cooperative file-path locks, not a database-wide or inode-based authority;
do not copy, hard-link, roll back or recreate the journal to continue an
experiment. A stale lock requires the existing manual process-termination and
checkpoint inspection procedure. This CLI never removes a stale lock for you.

The generation guard's `--admission` is optional on `method-generate-step`;
legacy calls preserve their previous behavior and are outside these cooperative
journal bounds. Separately, [`method-cost-assess --admission`](admitted-costs.md#cli)
is a read-only evidence join, not another generation admission path.
Neither this option nor the journal changes provider usage into trusted billing
or enforces input-token/aggregate wall limits. Loopback tests establish write
ordering and failure handling; they are not authenticated cloud evidence.
