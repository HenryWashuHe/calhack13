# Source-to-verified capability integration

The forge implements capability acquisition within GENESIS's larger organization
evolution loop. A newly accountable role can request a missing web capability;
the forge packages candidate source, creates a private Apify Actor, builds it,
executes the pinned build against independent cases, and returns a verified draft.
The kernel still decides whether the role and capability should become active.
See [server acquisition](SERVER_ACQUISITION.md) and [capability execution](CAPABILITY_EXECUTION.md)
for the merged kernel integration. The [original checkpoint](KERNEL_CAPABILITY_HANDOFF.md)
retains the historical branch/interface review.

## Implemented boundary

| Component | Responsibility |
| --- | --- |
| `genesis-forge::venue_source` | Assemble fixed production scaffold, dependency lockfile, exact URL policy and supplied parser; exclude tests and oracle answers |
| `SourcePackage` | Validate unambiguous paths and bounded UTF-8 files; hash the sorted manifest of exact path/length/content hashes |
| `genesis-apify` | Create a source-file Actor, check its source before building, check the build's captured `actVersion`, run and resume exact build references |
| `ForgeSession` | Freeze candidate and oracle identity; prepare one action, accept its matching completion, collect evidence and independently verify |
| `genesis-forge` CLI | Persist local private checkpoints before effects; provide prepare, step, status and export commands |
| Organization kernel | Durable database/outbox, leases, budgets, authenticated requests, role creation/activation, publication, adoption and invocation grants |

Candidate source can be supplied by an upstream author or generated through the
new [capability synthesis path](CAPABILITY_SYNTHESIS.md). Its bounded public request,
durable model call and `prepare-generated` handoff preserve exact source and model
provenance without exposing the independent oracle. The older baseline demo
remains hand-authored and labeled as such. Model generation and cloud build
adapters are tested against simulated HTTP responses; live account gates remain pending.

## Run locally

Requirements: Rust 1.94+, Node.js 24+ for the Actor scaffold, and a writable private
checkpoint directory. Run the full local checks from the repository root:

```sh
bash scripts/check-capability-core.sh
node scripts/demo-local-capability.mjs
```

Prepare a candidate with this metadata in `runtime/candidate.json`:

```json
{
  "operation_id": "venue-acquisition-001",
  "capability_id": "venue-extraction",
  "capability_version": "1",
  "actor_name": "genesis-venue-acquisition-001",
  "source_version": "0.1"
}
```

Create `runtime/` first. Supply `runtime/parser.ts` implementing the
[scaffold contract](../actors/venue-extractor-template/README.md). The default
scaffold parser always reports unsupported format; it is not a successful demo
parser. The fixture suite's `.example` URLs are intentionally offline placeholders.
For a cloud run, host the public fixture HTML and update the trusted suite's input
URLs and expected provenance consistently, retaining exact source-byte hashes.
Keep the oracle outside the source package and generated runtime.

```sh
cargo run --locked --manifest-path crates/genesis-forge/Cargo.toml -- prepare \
  --candidate runtime/candidate.json --extractor runtime/parser.ts \
  --suite crates/genesis-verification/examples/fixtures/trusted/suite.json \
  --checkpoint runtime/forge.json

cargo run --locked --manifest-path crates/genesis-forge/Cargo.toml -- status \
  --suite crates/genesis-verification/examples/fixtures/trusted/suite.json \
  --checkpoint runtime/forge.json
```

Preparation is offline. It derives only allowed URLs from the suite, includes
the exact parser bytes and production scaffold, validates the oracle, and refuses
to overwrite an existing checkpoint. Invalid metadata, duplicate JSON keys,
unsafe source paths and invalid oracle suites fail before cloud work.

For a live run, supply `APIFY_TOKEN` through the process environment and invoke
`step` repeatedly with the same suite and checkpoint. Each invocation performs
one state transition; polling may make several reads within the configured
deadline. The token is never a CLI argument or checkpoint field.

```sh
cargo run --locked --manifest-path crates/genesis-forge/Cargo.toml -- step \
  --suite runtime/live-suite.json --checkpoint runtime/forge.json
```

The live suite must be the same suite used during preparation. `step` targets
`https://api.apify.com`; the production CLI has no endpoint override. Exit 0 means
progress or accepted completion, 1 means rejection/failure/reconciliation, and 2
means invalid input or a local error. Always inspect the emitted phase.

After phase `verified`, `export` with the same suite/checkpoint emits a draft
containing artifact IDs, source manifest and a fresh verification report. Export
revalidates all receipts; editing a saved terminal label cannot establish acceptance.

## Persist before effects

Library integration follows this order under the kernel's exclusive job lease:

1. Restore the session from trusted storage and call `validate(&suite)`.
2. Call `prepare_next(&suite)` to record the pending action.
3. Commit the complete session checkpoint durably before executing the action.
4. Consume `action.execute(&client)` once and pass its completion to `complete`.
5. Commit the updated session and its domain event before acknowledging the job.

`ForgeAction` and its completion have private fields and cannot be deserialized.
The completion is bound to the candidate and suite digests, provider resource
context, action kind and sequence. A rejected completion leaves the session
unchanged. The
library does not supply the kernel's database transaction or distributed lease.

The CLI implements a single-machine version: an exclusive cooperative lock,
bounded JSON writes to a same-directory private temporary file, file fsync,
atomic installation/replacement, then directory fsync. On Unix, new checkpoint
and lock files use mode 0600. A crash leaves the lock for operator review. Only
after confirming the original process stopped should an operator remove that
lock. A lock is not a multi-tenant security boundary.

## Failure and recovery

| Condition | Behavior |
| --- | --- |
| Build or run polling fails transiently | Retain its ticket; retry the GET path with the CLI's explicit `--retry-read` after checking status/budget |
| Restart with a pending read | Resume the same resource; no new Actor, build or run submission |
| Lost response or restart with a pending POST | Mark `needs_reconciliation`; never automatically repeat the submission |
| Build source differs from submitted package | Fail before returning an executable artifact pin |
| Provider run succeeds but output contradicts the oracle | Reject the candidate; no verified draft |
| Suite or candidate changes after preparation | Reject the restored checkpoint; do not mix evidence across definitions |
| Semantic rejection with repair budget remaining | Library `repair` creates one new lineage member with new operation/version/Actor name/source and requires the entire suite again |

The CLI does not yet resolve unknown POST outcomes or automatically repair
parsers. Reconciliation must establish the provider resource and submitted source
before proceeding; editing a checkpoint to remove pending intent is not a recovery
procedure. Known tickets are resumable through the adapter. The kernel must enforce
global spending and repair budgets; a per-session repair counter is not that authority.

## Independent organization reuse

The result pins the source manifest digest and exact Actor/build ID and build
number. Org B can use `genesis-apify::RunRequest` with that pin, B's own input and
B's local source/oracle suite, then call `genesis_verification::verify` on its new
receipts. It does not need to upload source or build again. Tests cover distinct
input digests and run/dataset IDs while reusing the same artifact.

B must obtain the package from an authenticated publication path and separately
check local compatibility and authority. A deserialized report or manifest from
A is evidence, not an installation grant. Elastic discovery, package signatures,
organization state and adoption authorization belong to the integration layer.

## What the current checks establish

Rust tests exercise the real HTTP adapter against loopback servers, the state
machine under stale/corrupt/interrupted work, and the independent semantic gate.
TypeScript tests compile the runtime, fetch actual fixture bytes and use the real
Apify SDK's local dataset storage. These establish local behavior, not cloud API
compatibility in our account or a live generated capability.

The local demo command above prepares the exact source package through the forge
CLI, installs its lockfile in a fresh directory, compiles it once, and uses the real
SDK's local input/dataset storage for six executions across A and B. It feeds those
outputs into the Rust verifier, also proving that a changed capacity and A's old
receipts for B are rejected. Use `--output-dir NEW_DIRECTORY` to preserve the
source manifest, input/output files and evaluator reports; the directory must not
already exist. The demo explicitly uses synthetic provider IDs and a hand-authored
baseline, so its acceptance is not cloud execution evidence.

The parser runs inside the Actor's Node process. URL/runtime checks are guardrails,
not arbitrary-code isolation: parser code can import other APIs or loop. The Docker
base tag is mutable; source hashing alone does not attest the container image.
Retain the actual build reference and verify its outputs. Live Actor creation,
Docker build execution, cloud limits, costs and end-to-end sponsor integration
still require measured account-backed runs.
