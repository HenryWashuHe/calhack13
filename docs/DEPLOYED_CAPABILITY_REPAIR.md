# Repair an installed capability

GENESIS can use a verified semantic failure of an installed, locally generated
capability to seed one replacement acquisition. It reuses the existing model,
Actor build, execution verification, authority and quota machinery. Model weights
stay fixed. A changed parser is required, but a changed parser alone is not evidence
of improvement: the complete independent suite must accept the replacement.

## Evidence and authority

The request supplies an invocation job ID and parent acquisition ID. The backend
reconstructs the invocation proof from its collected receipt, exact installed
artifact, historical authority, accepted suite and frozen input. It independently
reverifies the parent acquisition and requires the same artifact and suite.
Neither caller assertions nor generated tests can authorize repair.

Only nonempty semantic failure categories are eligible: malformed or invalid
output, outcome mismatch, missing/unexpected/duplicate venues, and value mismatch.
Source, input, artifact and receipt mismatches, failed or uncertain runs, and
revoked authority are excluded. The model receives the previous generated parser
and coarse categories; case identities, expected answers and verification details
remain outside its prompt.

The original builder may have retired. A currently authorized role must own or
hold the assigned repair responsibility, with `capability.acquire`, a live mission,
lease and deadline. Use a new committed requirement/responsibility if the original
work is already satisfied. This endpoint neither creates a role nor expands grants;
the existing organization-change path handles a justified new specialist.

## API

All endpoints require the operator bearer token and configured acquisition policy.
Create a repair with `POST /api/v1/capability-repairs`:

```json
{
  "parent_acquisition_id": "acq_original",
  "failure_job_id": "job_failed_invocation",
  "acquisition": {
    "client_request_id": "repair-original-v1",
    "mission_id": "mission_current",
    "requirement_id": "requirement_repair",
    "role_id": "role_builder",
    "responsibility_id": "responsibility_repair",
    "suite_id": "configured-suite",
    "candidate": {
      "capability_id": "venue-extraction",
      "capability_version": "2.0.0",
      "actor_name": "venue-extraction-v2",
      "source_version": "0.1"
    },
    "repair": null,
    "allowed_source_urls": ["https://public.example/venues"],
    "examples": [],
    "deadline": null
  }
}
```

The IDs and URL above are placeholders. Supply the exact configured source and
committed organizational identities. Candidate capability ID must match the
parent; version and Actor name must differ. Operation IDs are backend generated.
A second intra-acquisition repair slot is prohibited.

Creation returns 201 with `acquisition_id`, `parent_acquisition_id`, `failure_job_id`,
`failure_proof_ref`, `parent_artifact` and the effective `deadline`. The same request
returns 200 with the original child, including after restart. A different request
using the same key, failure or exact parent artifact returns 409. The relationship,
child intent, quota debit and first history checkpoint commit in one transaction.
No provider call happens during creation.

Read the revalidated relationship at
`GET /api/v1/capability-repairs/{acquisition_id}`. Advance the returned child through
[the existing acquisition endpoints or automatic worker](SERVER_ACQUISITION.md).
An unchanged parser is rejected before Actor creation. A changed replacement gets
one fresh Actor/build and the complete suite. No second repair attempt follows a
failure of that replacement; uncertain effects remain in reconciliation. Count
quotas are organization-wide and are not refunded by a failed or interrupted run.

The existing `acquisition:<id>` publication path checks current authority and
reverifies the candidate. Neither publication nor repair changes existing installation
pins. Another organization must discover/select the replacement and perform its
own acceptance checks before using it.

## Bounds and remaining network work

This first path repairs a semantic failure on an already authorized frozen input.
It does **not** authorize new portal layouts or source snapshots. Layout drift needs
an explicit diagnostic-suite contract that preserves old acceptance evidence.

The parent parser must come from a local verified acquisition; registry manifests
currently do not provide authenticated retrievable parser source. Organization A
can repair its artifact and B can independently accept the replacement. The local
failure/parent/child relationship is durable, but authenticated shared repair
advisories, registry lineage, deprecation, rollback and automatic replacement
selection remain separate network milestones. A new child may itself become a
parent after it passes verification and a later invocation fails; its source
snapshot does not recursively embed every prior acquisition checkpoint.

The one-attempt rule is enforced by the repair endpoint per exact parent artifact.
It is not a claim that a trusted operator cannot create an unrelated acquisition.
Global acquisition quotas and authority checks still bound that separate path.

## Verification

`genesis-forge` acquisition tests exercise current-parent source reuse, terminal
unchanged/rejected replacements, bounded next-generation ancestry, tampered origin
rejection and byte-compatible legacy checkpoints. Server integration tests use real
HTTP adapters against model/Apify/registry fixtures, a file-backed organization,
and backend invocation proofs. They do not execute generated source on this host
or establish live sponsor results, unseen-layout adaptation or measured recursive
benefit. See [the roadmap and controlled evaluation](RECURSIVE_CAPABILITY_IMPROVEMENT.md).
