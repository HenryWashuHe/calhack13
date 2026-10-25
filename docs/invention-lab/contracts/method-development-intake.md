# Method selection and organizational responsibility

`POST /api/v1/missions/{mission_id}/method-development` accepts a receiver's
Operator/Agent token and strict JSON: `client_request_id`, `need`, `acceptance` and an explicit [MethodCandidate](method-registry.md).
Request IDs are 1–128 ASCII letters/digits or `-_.`; need/acceptance are nonempty, at most 2048 bytes each. HTTP bodies are limited to 16 KiB.

The configured registry client fetches and checks the complete selected identity
and canonical source. An active, unexpired receiver mission is checked both
before and after that read. Source is discarded; the full source-free request,
selection and request hash are retained atomically with one ordinary observation
and its `ObservationRecorded` event. The existing planner/Runner can consume that
event and choose existing ownership, justified specialist creation or no change.
The planner runs separately; intake itself creates no role. Concurrent initial requests may repeat a read-only fetch; only one observation commits.

HTTP 201 returns `genesis-method-development-intake/1` with receiver/mission,
observation ID, request hash and original request. Repeating the same request ID
and body returns 200 without another fetch or event, including after restart.
A different body or mission under that ID returns 409. Retrieve the retained
record with `GET /api/v1/missions/{mission_id}/method-development/{request_id}`.
Request IDs are unique across the receiver organization. A replay retrieves the
old receipt even if its mission later closes; it grants no new work or authority.
The observation contains the receiver's need/desired acceptance and a hash link
to the retained selection, not package source or publisher-authored instructions.
Hash consistency is not independent origin proof. Registry transport, ongoing
responsibility, local method acceptance and pinned invocation are separate steps.
This endpoint supplies no installation, acceptance, execution, provider-backed
specialist readiness, model judgment or proof of recursive improvement.
