# Backend capability execution and independent adoption

This integration connects the Rust organization API to the pinned Apify adapter,
forge checkpoint verifier and organization-local acceptance. It is part of GENESIS's
organizational evolution: a newly recruited role can evaluate a missing capability,
then another organization can install the same build and validate it independently.
The role/mission/change engine and UI remain Claude's workstream (issue #4).

## Start a configured organization

Run `genesis org --help` for token, constitution, database and registry options.
The additional options are repeatable:

```sh
cargo run -p genesis-server --bin genesis -- org \
  --org-id org_b --name 'Conference Co' --db data/org-b.db \
  --oracle-suite /absolute/path/org-b-oracle.json \
  --registry-url http://127.0.0.1:7100
```

Supply `GENESIS_OPERATOR_TOKEN`, `GENESIS_AGENT_TOKEN`, `GENESIS_ADAPTER_TOKEN`,
`GENESIS_REGISTRY_TOKEN` and `APIFY_TOKEN` through the process environment. The
Apify token has no CLI flag. For a publishing organization, add
`--forge-checkpoint /absolute/path/verified-checkpoint.json` and its associated
`--oracle-suite`. These files are trusted backend configuration, read with strict
JSON decoding and an 8 MiB bound. Startup recomputes checkpoint verification;
setting a checkpoint's phase to `verified` does not pass verification.

`--dev-stub` can supply missing role provisioners alongside a real Apify executor.
That fallback does **not** enable assertion-based capability publication or
acceptance when the executor is real. `genesis dev` is wholly a labeled simulation
and ignores `APIFY_TOKEN`.

## Publication and local evaluation

All organization paths below are under `/api/v1` and use the configured bearer
credential. Oracles and raw execution receipts are not uploadable through this API.

1. Org A's operator publishes with `POST /capabilities/publish`:
   `{"manifest":{...},"verification_ref":"<forge operation_id>"}`.
   The exact capability version, source digest, Actor, build ID and build number
   must match the configured verified checkpoint. The backend replaces the
   supplied verification report with freshly computed evidence.
2. Org B discovers the manifest and uses `POST /capability-installations` with
   `{"capability_id":"...","version":"..."}`. The immutable manifest is stored
   locally in the same transaction as the installation event. Status is `CHECKING`.
3. Under a currently assigned role and responsibility, B's operator starts
   `POST /capability-installations/<id>/checks` with
   `{"client_request_id":"check-1","role_id":"...","responsibility_id":"...","suite_id":"org-b-suite"}`.
   This queues one job per backend oracle case and returns `job_ids`. Evaluation
   requires the role's active assignment and every manifest permission. This
   narrowly scoped evaluation purpose can run before attachment; it does not grant
   general execution authority. It invokes the same published build, with zero
   source generation or repair calls.
4. The worker persists submission intent, submits the pinned build, saves the
   typed ticket, and collects native run/dataset metadata. `GET /capability-runs/<job_id>`
   shows progress and actual output, never expected oracle answers.
5. B's operator or backend adapter sends `POST /capability-installations/<id>/acceptance`
   with `{"suite_id":"org-b-suite","job_ids":["..."]}`. The server loads its own
   persisted receipts, checks installation/suite/artifact/input/source bindings,
   reruns the oracle verifier and rechecks current assignment authority. Only then
   does the kernel mark the installation `ACCEPTED` and save the proof reference.
   Jobs from another organization, another installation or a changed suite fail.
6. An organization change can now attach the installed capability to a role.

The role and responsibility must already be active before evaluation. Role creation,
handoff and retirement still go through the kernel's validated organization-change
protocol; a successful provider call does not create organizational authority.

## Invocations and recovery

`POST /capability-runs` takes `client_request_id`, `installation_id`, `role_id`,
`responsibility_id`, `input` and optional `required_scope` / `suite_id`. Production
runs require a locally accepted, attached capability and an exact input from that
accepted backend suite. The server recomputes local acceptance before dispatch.
This deliberately bounded fixture milestone does not claim oracle coverage of
arbitrary new web inputs. A changed request under the same key is rejected.

Job, request, idempotency key and dispatch events commit atomically. The runtime
checks role, assignment, epoch, deadline and required permissions before and after
provider I/O, without holding the kernel lock across that I/O. Completed collection
moves the job to `VERIFYING` and never writes a commitment. Semantic finalization
is a separate backend step described below.
General `/jobs/<id>/results` submissions cannot use a client boolean to complete
managed capability jobs. A future domain-action verifier must authorize any real
reservation or other external commitment separately.

A saved submission intent without a ticket becomes `needs_reconciliation` on
restart; it is never submitted again automatically. A saved collection intent keeps
its ticket and resumes with GETs only. Transient read failures retain the ticket and
require `POST /capability-runs/<job_id>/step` with `{"retry_read":true}`. Other
manual steps use `{"retry_read":false}`. There is currently no operator API for
resolving an ambiguous POST; investigate it using native provider records before
any new attempt. One organization process owns a database; this is not a
multi-process distributed lease implementation.

## Fresh invocation verification

After collection, an operator sends `POST /capability-runs/<job_id>/verify` with
exactly `{}`. `GET /capability-runs/<job_id>/verification` returns the same proof.
Both endpoints are operator-only. Finalization makes no network calls and accepts
no caller-supplied evidence, passed flag, oracle, or proof.

Preparing a new real invocation atomically freezes its installation, complete
oracle-suite digest, accepted report reference and immutable request fingerprint.
Finalization rechecks those bindings, locally collected evidence, current executor
assignment/epoch, lease, mission, deadlines, required permissions and attachment.
The full suite must still validate, even though this operation evaluates one case.
Its result type cannot become a full-suite publication or installation acceptance.

| Decision | Job status | Meaning |
| --- | --- | --- |
| `verified` | `SUCCEEDED` | Fresh output passes the selected case; `case_report.extraction` distinguishes `supported` from correctly `unsupported`. |
| `semantic_rejected` | `RESULT_REJECTED` | Output failed the selected oracle case; the report contains coarse issue categories. |
| `authority_rejected` | `RESULT_REJECTED` | Executor authority was lost before initial finalization; no semantic report is issued. |

The immutable response includes `job_id`, `installation_id`, `request_sha256`,
`suite_id`, `suite_sha256`, `acceptance_report_ref`, `org_revision`, `verified_at`,
`decision`, `case_report` and a domain-separated `invocation:<sha256>` proof
reference. It exposes no expected values, raw output or private oracle scope.
Terminal status, proof and events commit in one SQLite transaction. A successful
lookup does not write a ledger entry, book a venue or grant a new capability.
A correctly unsupported response is truthful extraction, not an available venue;
the inherited-mission driver requires a supported result before advancing.

Repeated verification is read-only. The server recomputes the proof using saved
authority revision and verification time, so executor retirement or expiry after
finalization does not erase historical evidence. Fresh successor jobs still need
current authority. The original acceptance verifier need not remain active.
Persisted evidence and the configured full suite must still match; changed oracle
policy is rejected rather than silently migrating an old proof. Digests detect
drift in trusted storage; they are not signatures against a hostile database writer.

Legacy runs without a frozen binding remain transport-only. Replaying their
submission cannot retrofit a binding; create a new invocation with a new request
key. Acceptance-purpose jobs cannot use this endpoint. Configuration/pin/binding
drift returns an error without changing status; lost authority on the first valid
finalization produces the immutable `authority_rejected` result instead.

## Trust and evidence limits

- Registry publisher credentials belong only to trusted verifier backends. The
  registry validates immutable manifests; it is not an independent cloud execution
  attestation service. Anyone holding a publisher credential is trusted to attest
  evidence. Do not give that credential to generated agents or browser clients.
- Local forge checkpoints are trusted storage. Strict parsing, source binding and
  fresh verification reject accidental tampering; they do not make attacker-authored
  checkpoint files authentic. Source snapshot digests are backend expectations,
  not cloud attestations of bytes fetched. Semantic verification checks returned
  source evidence against those expectations.
- Development assertions are marked `genesis:development-assertion` in provenance.
  A real executor refuses to install them. Production cannot adopt a legacy
  assertion-only installation without fresh backend acceptance.
- Existing databases made before local manifest pinning retain their history, but
  installations without a local pin cannot execute through the real path. There is
  no in-place migration endpoint in this milestone: install a new published version
  through this API, or use a fresh data directory for a disposable development stack.
- Tests use the actual HTTP adapter against loopback Apify protocol fixtures and
  separate organization databases. They do not prove access to a live sponsor
  account. Elastic, Fetch.ai and CREAO live integration remains separate work.

## Verification

```sh
cargo test --locked -p genesis-providers -p genesis-server
cargo clippy --locked -p genesis-providers -p genesis-server --all-targets -- -D warnings
```

The server capability tests exercise independent local receipts, publication and
acceptance gates, failure/restart behavior, revoked authority, request identity and
rejection of caller-supplied success claims. The existing development API scenario
continues to run with visibly labeled stubs.
