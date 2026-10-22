# Mission-bound capability acquisition

The organization server can own a complete acquisition rather than requiring a
caller to manage forge files. An operator creates an intent for an existing
requirement and assigned responsibility, then advances it through generation,
Actor creation/build, independent verification and one optional semantic repair.
The checkpoint and consumed action counts live in that organization's SQLite DB.

This builds on [CAPABILITY_ACQUISITION.md](CAPABILITY_ACQUISITION.md). It does not
create a role or choose an organization change. Claude's planner can call this
boundary after its change commits and the actual role, requirement and assignment
exist. Planner, provider-role activation, Elastic and UI remain Claude-owned.

## Configure trusted context

Acquisition is disabled by default, including in `genesis dev`. For `genesis org`,
provide `--acquisition-policy PATH` alongside the existing `--oracle-suite PATH`
and organization configuration. Start from
`crates/genesis-server/examples/acquisition/policy.json`; set a concrete model ID
available to your account. The policy contains no credentials or provider URLs.
`ANTHROPIC_API_KEY` and `APIFY_TOKEN` are environment-only. Missing clients allow
offline preparation/status but reject the relevant effect before changing intent
or consuming an effect count.

```sh
cargo run --locked -p genesis-server -- org \
  --org-id org_a --name 'Organization A' --db runtime/org-a.db \
  --oracle-suite runtime/trusted-suite.json \
  --acquisition-policy runtime/acquisition-policy.json
```

Set the existing operator/agent/adapter token configuration as described in
[DEV.md](DEV.md). Publication additionally needs the existing registry URL/token.
Use one process per organization DB. The supplied `.example` source URLs and HTML
are synthetic fixtures: host and consistently update public sources and the
independent suite before live Actor execution. No live model or Apify account run
is claimed by these local implementation tests.

## HTTP workflow

All endpoints below require the **operator** bearer token. The `suite_id` selects
an already configured backend oracle. HTTP requests cannot supply a model, oracle
answers, operation IDs, generation source, checkpoint or purported verification.
Unknown fields are rejected.

1. `POST /api/v1/capability-acquisitions` with the shape in
   `crates/genesis-server/examples/acquisition/request.json`. Replace the mission,
   requirement, role and responsibility IDs with committed organization IDs.
   Return: 201 for a new intent, 200 for the same idempotent request, 409 for an
   idempotency conflict or another active acquisition for that requirement/grant.
2. `GET /api/v1/capability-acquisitions/{id}` returns phase, attempt, artifact,
   authority/fencing status and organization count usage. It omits public-example
   HTML, generated code, provider outputs and the oracle. `currently_authorized`
   checks the current state; a subsequent step permanently records fencing when
   authority has changed.
3. `POST /api/v1/capability-acquisitions/{id}/step` with `{}` advances one local
   transition or one provider action. There is no automatic acquisition worker.
   Repeat deliberately while progress is allowed. A saved retryable read requires
   `{"retry_read":true}`; this flag never permits another mutating submission.
4. After `phase: verified`, use the existing
   `POST /api/v1/capabilities/publish` with a manifest and
   `"verification_ref":"acquisition:<id>"`. The server loads its own stored proof,
   checks current authority and recomputes verification against its configured
   suite. The manifest's artifact must match exactly. Client verification fields
   are replaced with independently derived evidence.

The generation requirement is constructed from the stored requirement summary and
acceptance text. Only those public-facing requirements, supplied public examples
and the explicit source allowlist enter generation. Oracle cases and answers do
not. Repair remains limited to the coarse categories described in the forge guide.
Requirement authors must not put private oracle answers in the requirement text.

Only an assigned, unsatisfied requirement with triggering observations in the
same active mission is eligible. Its open responsibility must reference that
requirement and share the accountable owner; a committed role must hold the
current assignment or delegation and the explicit `capability.acquire` scope.
The demo constitution recognizes this scope, but role grants still need to
include it. Preparing an acquisition does not grant authority.

## Fencing and recovery

The record captures organization identity, mission and requirement digests, role
authority, responsibility, assignment/epoch and lease. Its effective deadline is
the earliest of policy duration, requested deadline, mission deadline and lease.
Every effect and proof lookup rechecks that context. Handoff, retirement, closed
requirements, expired leases/deadlines or changed authority prevent further work.
Once fenced, restoring the old-looking grant cannot revive the record.

Before each network action, the pending checkpoint and count debit commit in one
SQLite transaction. Network calls run outside the kernel lock. Completion retains
provider IDs and evidence even if authority changed during the call, then fences
the result. This does not cancel an already transmitted provider request. A
process-local driver gate and database version/revision checks prevent conflicting
writes; they are not a distributed worker lease.

Startup recovery makes no provider calls. Unknown model/Actor/build/run POST
outcomes require reconciliation and are never replayed. Known pending GETs retain
their saved context and can resume on an explicit step. Credentials are not needed
for local recovery. A failed or interrupted acquisition cannot publish a proof.
Publication remains an explicit operator network action; authority is checked
before starting it, not atomically across the remote registry transaction.

## Count quotas and limits

The policy bounds total intents, simultaneous active acquisitions, model calls,
Actor creations, builds, verification-run submissions and read actions across
the organization's database lifetime. Every newly initiated action is debited
transactionally, including explicit read retries. An action may contain the
adapter's existing bounded polling; the counter is not a count of individual
HTTP GETs. Creating another request ID, retrying an intent or restarting the server
does not reset consumed counts. Terminal/fenced work releases active capacity.

Quota ceilings are frozen when acquisition is first enabled for the DB. Startup
rejects changed ceilings instead of silently resetting or reinterpreting usage;
changing them needs an explicit migration. A new model configuration or duration
can apply to new intents while old intents keep their captured configuration.
These are **organization-local action limits**, not an account-wide dollar cap.
Existing role/constitution budgets allocate authority, not measured provider spend.

The private record is capped at 9 MiB; its acquisition checkpoint retains the
8 MiB cap and 6 MiB pre-effect ceiling. Default provider bounds also apply to
normalized complete Apify evidence. Use the same forge binary and embedded Actor
scaffold for a DB retaining these acquisition records: validation rederives the
package even for a completed record. There is no scaffold migration/archive API
yet. Trusted exported ForgeSession proofs retain frozen package bytes.

The initial HTTP integration exposes status rather than adding new domain-event
schemas or UI behavior. Organization B still discovers the published manifest and
performs its own local acceptance against the pinned build; it cannot read or
step Organization A's private acquisition record. Neither generation nor a stored
verified phase authorizes capability installation.

## Checks

Tests use real Axum and provider HTTP clients against loopback servers. They cover
strict inputs, idempotency, durable counters/recovery, authority fencing, source
privacy, foreign proof rejection and the generation/forge integration. Run
`cargo test --locked -p genesis-server` for the server checks. These tests establish
coordinator behavior; live model quality, cloud builds and all-sponsor mission
activation remain separate demonstration gates.
