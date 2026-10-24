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
