# GENESIS Apify runtime

This crate executes the web capability a GENESIS organization has acquired. The
kernel supplies an immutable artifact pin and a trusted case input. The adapter
checks Apify build metadata, runs that exact build, polls the resulting run, and
returns `genesis_verification::ExecutionEvidence` for independent acceptance.
No kernel or UI state is owned here.

```rust,no_run
use genesis_apify::{ApifyClient, RunLimits, RunRequest};
use genesis_verification::{ArtifactPin, SourceSnapshot, ExecutionEvidence};
use serde_json::Value;

async fn execute_case(
    token: &str,
    artifact: ArtifactPin,
    case_id: String,
    input: Value,
    source: SourceSnapshot,
) -> Result<ExecutionEvidence, genesis_apify::Error> {
    let client = ApifyClient::new(token, RunLimits::default())?;
    client.execute(&RunRequest { artifact, case_id, input, source }).await
}
```

Create one reusable client in the backend and keep its token out of generated
Actor source, prompts, input, and logs. Production clients use only
`https://api.apify.com`. The separate local-testing constructor permits literal
loopback IPs over HTTP; use a synthetic token there. No redirect, automatic retry,
or implicit environment proxy is enabled. Authentication uses a sensitive Bearer
header, never a query parameter. Debug/error messages omit tokens and raw bodies.

## Execution contract

1. Read `GET /v2/actor-builds/{build_id}`; require matching `id`, `actId`, exact
   numeric `buildNumber`, and `SUCCEEDED`. A `latest` tag is rejected. The source
   digest remains the trusted forge's claim: Apify's build response does not
   authenticate GENESIS's source hash. [Build API](https://docs.apify.com/api/v2/actor-build-get)
2. Submit `POST /v2/actors/{actor_id}/runs?build={build_number}` once with the exact
   input and explicit timeout, memory, item, and charge limits. Request
   `forcePermissionLevel=LIMITED_PERMISSIONS`; this is not a complete arbitrary
   code sandbox. Defaults are 120 seconds, 256 MB, two items, and $1; provider
   billing semantics still apply. [Run API](https://docs.apify.com/api/v2/actors-runs-post)
3. Poll `GET /v2/actor-runs/{run_id}`. Every record must retain the run ID, Actor
   ID, build ID, and any observed dataset ID. Only `SUCCEEDED` can reach output
   collection; unknown statuses fail closed. [Run status API](https://docs.apify.com/api/v2/actor-run-get)
4. Fetch `GET /v2/datasets/{dataset_id}/items?format=json&limit=2&offset=0&clean=false`.
   Require **exactly one object**, which is the complete `ExtractionOutcome`
   envelope. Actors call `pushData(outcome)` once, not once per venue. Two items
   expose overflow instead of silently truncating it; any supplied pagination
   headers must agree. Byte limits apply while streaming, before JSON parsing.
   Duplicate JSON keys are rejected at every depth. [Dataset API](https://docs.apify.com/api/v2/dataset-items-get)
5. Construct receipt metadata from the trusted request and authenticated provider
   records. Only `output` comes from the Actor dataset. Pass the receipt to
   `genesis_verification::verify` with a trusted oracle suite; provider success
   does not establish extraction accuracy.

`input_sha256` hashes the exact JSON input submitted by this adapter. The backend
must supply the source snapshot corresponding to that case. This adapter neither
downloads the source nor proves that generated code actually fetched it. It
cannot authenticate deserialized receipts from arbitrary callers, issue execution
grants, establish cross-organization isolation, or authorize publication.

## Recovery and resource bounds

`RunLimits::deadline` bounds the complete operation, including requests, polling
sleep, and dataset reading. `request_timeout` also bounds each request. Defaults
cap input at 256 KiB and each response at 1 MiB; configurable caps never exceed
8 MiB. Input depth and node limits prevent unbounded hashing/serialization.

For durable jobs, call `submit(&request)` first. It validates the build and submits
once, then returns a serializable `RunTicket` immediately, without polling. Save
the ticket and original request in trusted kernel storage. After persistence or
restart, call `resume(&request, &ticket)`: it only makes GET requests, rechecks the
build and run identities, and collects output. The ticket binds the case, complete
artifact pin, source snapshot, canonical input digest, run ID, and initially
observed dataset ID. Changed request context is rejected before network access.
The ticket is a recovery record, not a signed authorization token; deserialize it
only from trusted storage. `execute` is the single-call convenience path when
durable recovery is not needed.

Errors include a stage, a known `run_id` when available, and
`submission_may_have_occurred`. If that flag is true, the kernel must reconcile
before submitting again. **A timeout or task cancellation does not cancel the
remote Actor.** The ticket enables recovery after receiving a submission response;
a process crash or lost response before durable ticket storage still requires
kernel reconciliation. This library does not implement a database, durable outbox,
automatic cancellation, account-wide rate limits, or a global budget. Those belong
to the kernel's job lifecycle. The client exposes explicit errors instead of
retrying an ambiguous side effect.

## Creating and building capabilities

`ActorDefinition` contains an Actor name, a two-part source version, and UTF-8
`SourceFile { name, content }` entries. Its `validate()` and `definition_sha256()`
methods perform local preflight with an 8 MiB serialized ceiling. The client's
`RunLimits.max_input_bytes` also caps uploads; raise its 256 KiB default for larger
lockfiles. Files use relative paths, reject duplicates and file/directory
collisions, and are sorted by path for a stable request digest.

`create_actor` uploads one `SOURCE_FILES` version to a **new private Actor** and
returns an `ActorTicket`. It requests limited permissions and no injected
environment variables. Inline files use `format: "TEXT"`; existing Actors are not
modified. An unscoped account token with Actor creation rights is needed for this
operation. [Create API](https://docs.apify.com/api/v2/actors-post)

Persist the Actor ticket before calling `start_build`. The adapter first reads
the version and compares the complete file set and contents. A mismatch stops
before the build POST. [Version API](https://docs.apify.com/api/v2/actor-version-get)
The build request supplies the exact source version, disables cache and beta
packages, and returns a `BuildTicket` immediately. Save it before calling
`resume_build`, which makes only GET requests. Build submission does **not** have
the run API's timeout/memory/charge parameters; local request and polling deadlines
do not cap provider build cost or stop a remote build. [Build API](https://docs.apify.com/api/v2/actors-builds-post)

A successful build must retain its Actor ID, build ID, exact build number, and
matching `actVersion` source snapshot. Missing source files, changed content,
unknown filenames, changed source versions, or unknown status never produce a
`BuiltActor`. The completed value exposes getters for those identifiers and the
canonical definition digest. It is not deserializable as a success claim, and it
does not establish extraction accuracy. [Build status API](https://docs.apify.com/api/v2/actor-build-get)

Both tickets are serializable records for trusted backend storage. Their
fingerprints bind the request, not authority or a cryptographic provider
attestation. Forge errors preserve safe known Actor/build IDs and flag possibly
successful submissions; lost POST responses require reconciliation before any
retry. Every explicit `create_actor` or `start_build` call is a new side effect;
the kernel must prevent duplicate dispatch. No POST is retried automatically.

## Local validation

```sh
cargo test --locked --manifest-path crates/genesis-apify/Cargo.toml
cargo clippy --locked --manifest-path crates/genesis-apify/Cargo.toml --all-targets -- -D warnings
```

Native loopback HTTP tests exercise wire-level pinned submission, provenance
checks before and during polling, unknown/failing runs, timeout recovery,
malformed/duplicate JSON, oversized or ambiguous datasets, redirection, credential
redaction (including provider-echoed secrets), bounded input serialization, ticket
round trips, and restart recovery without resubmission. A complete mock-provider
pipeline runs every shipped oracle case through the adapter and verifier, then
confirms that incorrect output from a successful run is rejected. Tests use no live credentials or Apify
account. Authenticated cloud smoke testing remains required before claiming
provider integration has been validated.

Official API contracts were reviewed on October 21, 2026. The crate depends on
the adjacent `genesis-verification` crate and deliberately does not create or
modify the root Cargo workspace while the kernel is developed in parallel.
