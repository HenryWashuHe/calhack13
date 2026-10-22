# Durable capability acquisition

For organization-owned SQLite persistence, live grant checks and HTTP endpoints,
see [SERVER_ACQUISITION.md](SERVER_ACQUISITION.md). The standalone CLI below remains
available for explicit operator-managed checkpoints.

`AcquisitionSession` joins public-requirement synthesis, exact Actor packaging,
Apify builds and independent verification in one durable workflow. A preplanned
repair can replace a semantically rejected parser once. The original generation,
build, run evidence and repair lineage remain in the checkpoint. This is the
capability action for a planner-created responsibility; it does not itself create
or activate that responsibility.

The coordinator composes [source synthesis](CAPABILITY_SYNTHESIS.md) and the
[forge](FORGE_INTEGRATION.md). It uses their real HTTP clients, with synthetic
responses from local servers in tests. No paid model call or cloud Actor build/run
was made to validate this milestone. Claude retains planner, asynchronous role
activation, Elastic and UI ownership.

## Prepare and drive

Start from `crates/genesis-forge/examples/acquisition/plan.json`. Replace the
placeholder model with a concrete model ID available to your account. `request`
contains only public requirements, examples and allowed URLs; `model` is explicit;
`repair` is either null or a preselected replacement operation and candidate.
The replacement must retain the capability ID and use a different operation ID,
Actor name and capability version. The source version may stay the same because
the replacement is a different Actor.

The supplied HTML is synthetic and the `.example` URLs are offline placeholders.
Host the public pages and update the request and independent suite consistently
before a real run. Keep held-out variations in the suite to evaluate behavior
beyond the supplied samples. The model never receives the suite.

```sh
mkdir -p runtime
cp crates/genesis-forge/examples/acquisition/plan.json runtime/acquisition-plan.json
# Edit runtime/acquisition-plan.json with your model and hosted public sources.

cargo run --locked -p genesis-forge -- acquire-prepare \
  --plan runtime/acquisition-plan.json \
  --suite crates/genesis-verification/examples/fixtures/trusted/suite.json \
  --checkpoint runtime/acquisition.json

cargo run --locked -p genesis-forge -- acquire-status \
  --suite crates/genesis-verification/examples/fixtures/trusted/suite.json \
  --checkpoint runtime/acquisition.json
```

Prepare and status are offline. Supply `ANTHROPIC_API_KEY` and `APIFY_TOKEN` through
your normal environment/secrets mechanism for generation and forging respectively:

```sh
cargo run --locked -p genesis-forge -- acquire-run \
  --suite crates/genesis-verification/examples/fixtures/trusted/suite.json \
  --checkpoint runtime/acquisition.json --max-steps 32
```

`acquire-run` requires a bound of 1–64 transitions and emits one compact JSON
status per transition. `acquire-step` advances at most one transition. A transition
is either local work or one checkpointed model/Apify action. A run may stop while
a build is still running; inspect its phase and resume the same checkpoint.
There is no background worker, polling delay or automatic retry of failed reads.
Use `--retry-read` only after inspecting a saved retryable read failure.

Exit 0 means success or bounded progress, **not necessarily completion**. Only
`phase: verified` and a successful reverified export mean the suite passed. Exit 1
means rejected, failed, reconciliation required, or a saved retryable read failure;
exit 2 means invalid input or a local error. A step or bounded run may stop at the
original rejection before entering its planned repair. The next invocation can
advance that repair if it is eligible. At most two generations and two distinct
Actor/build verification attempts are permitted across all resumptions.

After verification, export the draft or a backend-compatible forge proof:

```sh
cargo run --locked -p genesis-forge -- acquire-export \
  --suite crates/genesis-verification/examples/fixtures/trusted/suite.json \
  --checkpoint runtime/acquisition.json

cargo run --locked -p genesis-forge -- acquire-export-forge \
  --suite crates/genesis-verification/examples/fixtures/trusted/suite.json \
  --checkpoint runtime/acquisition.json --output runtime/verified-forge.json
```

The output file must not exist. The exported `ForgeSession` is accepted by the
existing backend proof configuration described in
[CAPABILITY_EXECUTION.md](CAPABILITY_EXECUTION.md); the acquisition envelope is
not a replacement for that file format. Both exports independently recompute
verification. Neither grants publication or installation authority. Organization B
still has to run its own local acceptance against the exact published build.

## Repair policy

Only a rejected, fully collected verification attempt with exclusively semantic
issues can trigger the single planned repair. Model or provider failures, unknown
submission outcomes, failed builds, and artifact/input/source/receipt mismatches
never trigger it. Repair receives the original public request, prior parser and a
deduplicated, fixed-order subset of seven categories:

`malformed_output`, `invalid_output`, `outcome_mismatch`, `missing_venue`,
`unexpected_venue`, `duplicate_venue`, `value_mismatch`.

No oracle answers, report messages, paths, case IDs, counts, source hashes or
provider outputs are included. These categories are deliberate coarse feedback,
not a zero-information boundary. The new parser hash must differ from the first;
a new model message or provenance file alone cannot qualify as repair. A changed
parser is uploaded to a fresh Actor, built and run against every oracle case again.
There is no third attempt and no promotion based on a provider success flag.

## Planner/library contract

Create `AcquisitionSession::new(plan, suite)` with a trusted independent suite.
On restoration call `validate(suite)` and `recover_interrupted(suite)`, saving any
recovery transition. Call `advance_local(suite)` until it returns false before
treating a rejected phase as terminal. For generation use `begin_generation`,
durably save, invoke the model and call `complete_generation`. For forging use
`prepare_forge`, durably save, execute the returned action and call `complete_forge`.
Persist completion before advancing. Use `verified(suite)` for authority-bearing
evidence, never a serialized phase or a caller-provided passed flag.

The CLI writes compact, private, atomically replaced checkpoints with file and
directory fsync, and holds a cooperative lock through each request and save.
Interrupted mutating submissions require reconciliation and are never replayed.
Known read actions can resume; recorded read failures require explicit retry.
A crash may leave a `.lock` file: confirm that process has stopped before removing
the lock, then resume normally. Do not edit pending operations to force a retry.

The full compact checkpoint is capped at 8 MiB. Before an effect, both library
and CLI enforce a 6 MiB ceiling, reserving space for the default clients' bounded
responses. The Apify limit covers both wire responses and the normalized complete
execution evidence; numeric expansion during JSON serialization cannot bypass it.
A library caller changing provider response limits must supply an
equivalent storage/headroom policy. Use the **same forge binary and embedded
Actor scaffold until acquisition finishes**: validation rederives the expected
package, and a scaffold upgrade fails closed rather than migrating a pending
checkpoint. Exported ForgeSession proofs retain their frozen source package.

The future mission integration must bind acquisition to a live requirement,
assignment, grant, lease and budget; recheck these before effects and publication;
run network work outside the kernel lock; and retain the acquisition ID in its
ledger. This coordinator has no account-wide spending ledger, organization role
authority or scheduler. Checkpoint hashes detect drift, not hostile edits to
trusted storage. Generated code remains untrusted and is never executed on the
host by this workflow; the Actor scaffold is not arbitrary-code containment.

## Verification

`cargo test --locked -p genesis-forge` covers local state, real client interactions
with loopback providers, full repair and second rejection, interrupted effects,
all-case reruns, prompt separation, unchanged parsers, linked evidence tampering,
private CLI storage, bounded runs and export refusal. These tests prove coordinator
behavior, not model quality, a live Apify build or end-to-end sponsor activation.
