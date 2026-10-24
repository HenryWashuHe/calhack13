# Capability gap finalization (`acquired` → `installed`)

The gap driver ([CAPABILITY_GAP_POLICY.md](CAPABILITY_GAP_POLICY.md)) stops at
`acquired`: an independently verified acquisition, not yet published,
installed or usable. The finalizer takes that exact artifact the rest of the
way inside the same organization, so a committed `RequestCapability` can end
in an installed capability with no manual calls. The kernel's final gap note
(#57) then wakes the planner to attach it.

Code: `crates/genesis-server/src/capability_gap_finalizer.rs`.
Tests: `crates/genesis-server/tests/gap_finalizer.rs`.

## Turning it on

Off by default. Add to the configured `genesis org` command, next to the gap
driver's flags:

```bash
genesis org ... --auto-acquire --acquisition-policy acquisition.json \
  --gap-acquisition-policy gaps.json \
  --gap-finalization-policy finalize.json \
  --registry-url https://registry.example --registry-token ...
```

It requires gap acquisition, a capability registry and a real Apify executor
(local checks run the published build). `finalize.json` is trusted operator
configuration, one rule per capability ID:

```json
{"rules": [{
  "capability_id": "venue-reader",
  "suite_id": "offline-venue-suite-v1",
  "intent": "Read venue capacity, price and availability from the public portal",
  "applicability_conditions": ["Only the configured oracle scope"],
  "known_limitations": ["Lookup only; no reservation"],
  "input_schema": {"type": "object"},
  "output_schema": {"type": "object"},
  "required_permissions": ["capability.venue_lookup"],
  "responsibility_template": "Keep verified venue lookup available"
}]}
```

`suite_id` is the backend oracle suite for local acceptance. The text becomes
the published manifest; the planner and HTTP clients cannot supply any of it.
A gap whose capability has no rule stays `acquired` for an operator.

## What it does

For each `acquired` gap, one stage per pass (the worker runs every second):

1. **Freeze** one record: the acquired artifact pin (recomputed from the
   acquisition's proof), the exact manifest built from the rule, the suite and
   its digest, and the requesting role's current assignment and epoch. A
   changed rule, manifest or suite afterwards stops at attention.
2. **Publish** through the existing trusted path (`verification_ref:
   acquisition:<id>`; the server recomputes the evidence), then read the
   registry back and require exactly this pin, publisher, permissions and a
   passing report. Only then is `publication_ref` (`registry:<id>@<version>`)
   recorded on the gap and the record, in one transaction.
3. **Install** the confirmed manifest locally (`begin_installation` and its
   pin, with the record, in one transaction). After an interrupted pass, an
   installation of exactly this pin is reused, never one owned by an adoption.
4. **Check**: queue the existing local-acceptance jobs (`gap-finalize:<gap>`
   key, one per suite case) with the record, in one transaction. The
   capability worker owns every provider effect.
5. **Accept**: once every run is collected, recheck each job's authority as
   the acceptance endpoint does, verify the runs against the suite, and in one
   transaction mark the installation `accepted`, save the proof, record the
   gap `installed` (the kernel checks the installation and version) and close
   the record.

Status (operator token): `GET /api/v1/capability-gaps/{id}/finalization`
returns state (`active`, `attention`, `installed`, `refused`), an attention
code, the capability, `publication_ref`, `installation_id` and check job IDs.
It never returns the manifest, suite or evidence.

## Failure handling

| Situation | Result |
| --- | --- |
| Rule, manifest or suite changed after freezing | `attention: configuration` |
| Acquisition no longer verified, or the gap no longer names it | `attention: acquisition` |
| Publication failed and the registry does not hold exactly this pin | `attention: publication` |
| Publication reply lost but the registry holds exactly this pin | confirmed and recorded; no second version |
| Installation can't be created, or an existing one doesn't match | `attention: installation` |
| A check run failed, has a retryable read failure, or its outcome is unknown | `attention: checks`; jobs and installation kept, nothing resubmitted |
| Local verification rejects the outputs | gap `refused`, installation closed `rejected`, one transaction |
| Role's assignment, epoch or permissions changed, or the live requirement changed or is no longer assigned | gap `refused`; a still-checking installation is closed `rejected` |
| An accepted installation of this pin exists with a different suite, a report not backed by its stored proof, or runs that no longer verify | `attention: installation` (never reused) |
| Storage error during a pass | logged; that gap is retried next pass, others continue |

Attention is sticky. `POST /api/v1/capability-gaps/{id}/finalization/retry`
(operator) returns the record to `active`; the next pass re-reads every child
and decides again. It replays nothing: check jobs keep their keys, an unknown
or failed run stops at attention again, and a repeated publication can only
return the stored immutable version (the registry compares the whole manifest
apart from timestamps).

Refusal is used only for a definite answer (semantic rejection, lost
authority). Unknown outcomes are never turned into refusals.

## Ownership and fences

Once the finalizer records an installation, that installation is
finalizer-owned: the public `/checks` and `/acceptance` endpoints refuse it
with `409 gap_finalizer_owned`, so acceptance stays the finalizer's atomic
decision. The capability worker also checks the finalizer before submitting a
check run and before applying its result: a run on a finalizer-owned
installation is fenced three ways (`run_fence`):

- **allow**: the record is active, its frozen assignment/epoch is current and
  the live requirement is still assigned with the same digest
  (`engine::requirement_digest`);
- **pause**: the record is at attention but its authority is still current.
  The worker makes no provider call and leaves the run's phase alone, so an
  operator retry resumes the same check jobs. An answer already in flight is
  still recorded (an ambiguous submission stays `needs_reconciliation`);
- **revoke**: the grant or live requirement changed. The run is rejected
  before any call, and an answer that arrives afterwards is not applied.

Authority is "current" when the same role holds the same assignment and epoch
for the responsibility, the mission is active and in time, the role holds every
manifest permission, and the live requirement is unchanged and still assigned.
An already accepted installation (an interrupted pass, or an operator's earlier
acceptance) completes the gap only if its stored proof is for this record's
exact suite, the installation's report is that proof's report, its runs still
verify against the frozen pin, and every proof job is authorized now.
Publication is confirmed only when the registry's manifest equals the frozen
one apart from fields publication itself sets (recomputed verification, its
suite reference and provenance, state and timestamp).

## Why not adoption?

Adoption is Org B's independent discovery: it searches the registry, excludes
its own publications and may pick any compatible candidate. Finalization must
install the exact artifact that was verified for this gap, so it reuses the
lower-level local install/check/acceptance seams instead, and Org B's policy is
unchanged.

## Evidence

`tests/gap_finalizer.rs`, real kernel commits and SQLite, a real registry
server, real Rust Apify and model clients against a loopback provider:

- a verified gap is published, installed, checked and recorded installed; the
  pin, publication proof, one check batch, zero extra model/Actor calls and
  exactly one planner note are asserted; further passes change nothing;
- reopening the organization between every stage reaches `installed` with one
  installation and one batch;
- concurrent steps keep one record, installation and batch;
- without the manifest's permission the gap is refused before publication and
  no check runs are made;
- a changed rule stops at attention; status is operator-only and redacted;
  retry re-reads and stops again;
- an ambiguous check-run submission stays attention with IDs kept and is never
  resubmitted;
- a semantic rejection refuses the gap and closes the installation;
- a publication whose reply was lost is confirmed, not duplicated;
- a reassignment after installation refuses and closes it, with no checks;
- `serve_org` alone takes a committed gap from `open` to `installed`.

No live provider was contacted.
