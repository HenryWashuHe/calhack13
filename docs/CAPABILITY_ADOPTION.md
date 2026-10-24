# Durable independent capability adoption

An organization can now request a capability from its own unmet requirement and
keep that adoption progressing across server restarts. The request supplies the
local mission, requirement, executor, responsibility, and trusted suite selector.
It does not supply another organization's artifact, a model instruction, a passed
flag, or a verification checkpoint.

The organization derives the search text from the persisted requirement and
filters the shared registry's ranked results using its current scopes. It checks
at most five candidates, fetches the canonical manifest, and freezes one eligible
foreign capability. The existing registry search is the retrieval boundary;
Elastic can replace its keyword implementation without changing adoption. This
coordinator does not infer requirements, rank with another model, create roles,
or attach capabilities to roles. Those remain planner and organization-change
operations.

## HTTP contract

All adoption routes require the organization's operator token:

- `POST /api/v1/capability-adoptions` creates an intent. The body contains
  `client_request_id`, `mission_id`, `requirement_id`, `role_id`,
  `responsibility_id`, `suite_id`, `provider_access_acknowledged: true`, and an
  optional `deadline`.
- `GET /api/v1/capability-adoptions` lists the bounded retained intents.
- `GET /api/v1/capability-adoptions/{id}` returns its bounded status.
- `POST /api/v1/capability-adoptions/{id}/step` accepts only `{}` and advances
  one coordinator stage. Normal operation uses the worker started by `serve_org`.
- `GET /api/v1/capability-adoptions/{id}/candidates` returns why each registry
  candidate was or wasn't used (see below).

## Candidate decisions

A registry search ranks candidates by similarity. Rank and score only order
them; they never make a candidate applicable. For every candidate in the
search answer, discovery records the exact `capability_id`, `version`,
`publisher`, `source_digest`, required permissions, the publisher's
`applicability_conditions` (display only) and this organization's decision:

| Decision | Meaning |
|---|---|
| `own_publication` | published by this organization; adoption only inherits from others |
| `missing_scope` | needs permissions the requesting role doesn't hold (`missing_scopes` lists them) |
| `invalid_manifest` | the manifest disagrees with the search answer or fails local checks; discovery stops |
| `unusable_state` | the manifest can't be installed; discovery stops |
| `manifest_unavailable` | the manifest couldn't be read: applicability unknown; discovery stops |
| `invalid_response` | the registry answer as a whole was invalid; nothing in it was evaluated |
| `selected` | first eligible candidate by rank; sent to this organization's own acceptance checks |
| `adopted` | this organization's acceptance suite accepted it (`acceptance_ref`) |
| `not_applicable` | this organization's acceptance suite rejected it (`acceptance_ref` and a summary of failed cases and issue kinds, never expected values) |
| `not_evaluated` | a higher-ranked candidate was selected, or discovery stopped first |

Decisions are written in the same transaction as the discovery outcome they
explain, and the suite's verdict replaces `selected` in the transaction that
finishes the installation, so a restart or repeated step can't record a
different story. A registry that can't be searched records nothing: the
outcome stays unknown (`registry_unavailable`) and never becomes an empty
result. The candidate's free-text intent is not copied, since it can echo the
private requirement query. Adoptions made before this log existed have no rows.

The client request key is durable. Repeating an identical request returns the
same intent; changing its binding returns a conflict. An organization retains at
most 32 intents, allows at most four active intents, and limits their lifetime to
30 minutes. The backend must have a registry, an Apify executor, and the selected
trusted suite configured. Acquisition and model credentials are unnecessary.
The record bounds are 256 KiB per intent, 128 KiB for the selected manifest,
4096 bytes for the requirement query, and 256 bytes for exact UTF-8 domain IDs.
The suite uses the verifier's existing limits; the intent retains its digest.
The current role scopes are part of the frozen authority binding.

## Execution and recovery

Discovery saves the selected manifest and installation in the same transaction.
The next transaction records the complete local acceptance batch, all child jobs,
and the intent's checking stage. A lost HTTP reply or restart therefore cannot
leave an unowned installation or create another batch.

Discovery records a claim before its read-only registry requests. At most three
claims are allowed across interruptions and restarts. An interrupted discovery
may repeat these reads within that bound; a returned registry failure stops at
attention without automatic retry.

The existing capability workers execute each trusted suite input against the
selected exact Actor build. Every child job deadline is capped by the adoption
intent deadline. Adoption never creates an Actor, requests a build, or generates
source. The suite is B's own backend configuration; the
publisher's report only supports discovery. Successful local verification commits
the accepted installation, its local evidence reference, and the adoption state
atomically. A real semantic mismatch rejects the installation. Neither result
writes a booking or other business commitment.

Provider failures stop automatic adoption. In particular, an unknown run POST
outcome does not cause a replacement run, batch, candidate selection, or synthesis.
Adoption does not automatically request read retries. Status identifies work
requiring attention; it does not imply that a provider effect failed to occur.
Current requirement, assignment, epoch, lease, mission, and declared permissions
are checked before live work and when collecting its result. Revoked authority
cannot create a new acceptance.

Terminal attention or fencing also closes that intent's still-checking
installation as `Rejected`, in the same transaction as the terminal intent.
Its reference is `adoption-stopped:<id>:<version>`, and its static reason records
that verification stopped without acceptance while provider effects may still
exist. This is not an oracle verdict about
the parser: the intent stays `Attention` or `Fenced`, its semantic `rejection_ref`
is empty, and no local acceptance proof is stored. Already accepted installations
are never closed by this path. Replaying the terminal intent changes nothing.

Child records and known provider tickets remain retained. An unknown earlier
POST may still have taken effect; closing the installation neither cancels nor
reconciles it. An operator can explicitly create a new intent with a new request
key and current authority. It may select the same capability into a distinct
installation and authorize fresh work. This is not an automatic retry of the old
submission, and it does not discard the old reconciliation evidence.

Committed acceptance belongs to the receiving organization. It remains available
after the verifier retires or the process restarts. A successor still needs its
own active assignment, permissions, and an explicit `AttachCapability` organization
change before invoking it. Adoption does not resolve the requirement or grant
these bindings automatically.

## Demonstration boundaries

The integration fixtures use separate registry and organization HTTP servers, a
real Rust Apify client, and canned loopback provider responses. The publisher's
fixture summary is computed by the verifier and remains advisory; B executes
fresh checks with different inputs and its own suite. They do not claim live
cloud execution, generated code execution, or brokered access between accounts.

`provider_access_acknowledged` records the operator's acknowledgment that the
receiving organization already has permitted access to the selected Actor. It
transfers no credentials and changes no Actor visibility. Private Actor access
must be arranged outside this coordinator.

This first version selects one candidate and executes one acceptance batch. It
has no automatic candidate fallback, no synthesis fallback, and no automatic
retry of a failed provider read. Accepted proof validation continues to require
the same trusted suite configuration. Fresh invocation remains limited to exact
inputs represented in that configured suite. These are existing evidence-policy
boundaries, not claims of general-purpose capability correctness.

## Regression evidence

`crates/genesis-server/tests/capability_adoption.rs` exercises eight integration
cases. The create-only case discovers a foreign capability despite self-owned
and permission-incompatible distractors, runs B's independent suite, retires the
verifier with a responsibility handoff, restarts B, and automatically verifies a
fresh invocation by its successor. B never has a model client or an acquisition
configuration.

Other cases cover a discarded create reply and idempotent replay, operator-only
strict requests, restart after installation and batch creation, semantic
rejection, an ambiguous provider POST across restart, revoked execution authority,
and a changed requirement or trusted suite before the adoption worker can react.
SQLite abort triggers verify that installation, all child jobs, and local
acceptance each roll back if saving the corresponding intent stage fails. A
separate acceptance request cannot bypass the adoption transaction. Terminal
closure has its own rollback check. The ambiguous-POST case also preserves its
old child record across restart and a fresh explicit selection of the same
capability into a different installation, with no additional provider submission
until new jobs are authorized.
