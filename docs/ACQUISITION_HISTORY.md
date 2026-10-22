# Acquisition history

The acquisition journal gives operators a bounded view of what an organization
has attempted and where it needs attention. It records metadata about acquisition
transitions and scheduler attention. It does not execute work, retry a provider
request, or change an organization's authority.

History is observational metadata. A recorded `verified` phase is not a verification
proof, a publication credential, or permission to act. Execution and publication
continue to use the existing backend verification and authority checks described in
[SERVER_ACQUISITION.md](SERVER_ACQUISITION.md).

## Read endpoints

Send the organization's operator bearer token. Agent and adapter tokens cannot
read these endpoints, and there is no caller-selected organization parameter.

| Endpoint | Result |
| --- | --- |
| `GET /api/v1/capability-acquisitions` | Acquisitions ordered by their immutable insertion ordinal, with a metadata snapshot for each item. |
| `GET /api/v1/capability-acquisitions/{id}/history` | Append-only journal entries for one acquisition, ordered by journal sequence. |

Both endpoints accept `after` and `limit`. `after` defaults to `0` and must be an
integer between `0` and `9223372036854775807`. `limit` defaults to `25` and must be
between `1` and `100`. The collection endpoint also accepts exact `mission_id` and
`requirement_id` filters; use the existing identifiers returned by the organization.
When both filters are supplied, both must match. Filter IDs must be nonempty,
contain only ASCII letters, digits, `-`, `_`, `.`, or `:`, and cannot exceed 256
characters. Unknown query parameters, duplicate fields, malformed numbers and
out-of-range values are rejected.

For example:

```text
GET /api/v1/capability-acquisitions?after=0&limit=25&mission_id=mission-example
GET /api/v1/capability-acquisitions/acquisition-example/history?after=0&limit=25
Authorization: Bearer <organization-operator-token>
```

No model client, Apify client, acquisition policy, or automatic-worker configuration
is required to read an already initialized journal. This permits inspection after
provider credentials are removed or automatic progression is disabled. If the
journal is not fully initialized, including an existing acquisition awaiting a
validated migration baseline, the endpoints return `503` rather than imply that
an unavailable history is empty. A fresh organization has an empty ready journal.

## Pages and cursors

Both responses have `items` and `next_after`. An empty page is:

```json
{"items": [], "next_after": null}
```

A collection item contains `ordinal`, `snapshot`, and optional `attention`. A
history entry contains `seq`, `kind`, `checkpoint_org_revision`, `recorded_at`,
`snapshot`, optional `debit`, and optional `attention`.

Each snapshot contains:

- Organizational bindings: `id`, `org_id`, `mission_id`, `requirement_id`, `role_id`,
  `responsibility_id`, `assignment_id`, and `epoch`.
- Acquisition state: `version`, `created_at`, `updated_at`, `phase`,
  `generation_phase`, optional `forge_phase`, `attempt`, optional `pending_action`,
  `completed_case_count`, optional `fence_reason`, and `active`.

These are bounded metadata fields, not the underlying acquisition checkpoint.
`checkpoint_org_revision` supplies historical context: a baseline records the
organization revision observed during migration, and attention entries retain
their acquisition version's checkpoint revision. It never asserts current
authority or that the organization still has that revision.

Use `next_after` unchanged to request the next page with the same filters. It is
present only when more matching items were observed and equals the last returned
item's cursor. Collection cursors are insertion ordinals, so an acquisition changing
phase does not move it between pages. History cursors are organization-wide journal
sequences filtered to one acquisition; gaps between its entries are normal.

Pagination does not freeze the organization. New entries can be appended and
collection metadata can change between requests. `next_after: null` means there
were no more matching items at that read, not that the acquisition has finished.
For later polling, retain the last observed `ordinal` or `seq` and request items
after it; preserve the prior cursor when a page is empty.
Polling the collection after an ordinal discovers newer acquisitions, not updates
to older ones. Follow each acquisition's history cursor to observe its transitions.

## History and disclosure boundaries

The journal is append-only. State transitions and scheduler attention are separate
observations; an attention entry does not invalidate an earlier receipt or authorize
a retry. Use the current acquisition and automation endpoints to inspect what the
runtime can do next.

Entry kinds are `created`, `transition`, `baseline_observed`, `attention`, and
`attention_cleared`. A recorded quota debit is one of `model_call`, `actor_create`,
`build`, `test_submit`, or `read`; it records reservation of an effect intent, not
proof that a provider completed the effect. Attention codes are `retry_read`,
`quota`, `configuration`, `invalid_record`, `conflict`, and `internal`.

When an existing acquisition predates the journal, initialization records only a
baseline of the metadata available at migration. It does not reconstruct earlier
provider calls, transition times, errors, or a complete creation history. A baseline
must not be presented as evidence that every preceding step was observed.

History excludes generated parser/source contents, public HTML examples, oracle
answers, provider response bodies, credentials, and native Actor/build/run/dataset/message
identifiers. It also excludes provider error text. These endpoints neither expose
nor accept execution evidence or proof objects. Scheduler attention uses a closed
error-code vocabulary instead of raw provider or database messages.

The read handlers return static errors:

| HTTP status | Error code | Meaning |
| --- | --- | --- |
| `400` | `invalid_history_query` | Malformed, duplicate, unknown, or out-of-bounds query fields or IDs. |
| `401` | `unauthorized` | Missing or invalid credentials. |
| `403` | `missing_authority` | The authenticated principal is not an operator. |
| `404` | `not_found` | The requested acquisition history does not exist in this organization. |
| `503` | `acquisition_history_unavailable` | The journal is not fully initialized or awaits a validated migration baseline. |
| `500` | `internal` | Stored history could not be validated or read. |

Storage failures do not return database error strings, source data, or checkpoint
contents. A collection with no matching acquisitions returns an empty successful
page rather than `404`.
