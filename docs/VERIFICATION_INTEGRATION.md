# Capability acceptance integration

`crates/genesis-verification` implements a synchronous, deterministic Rust acceptance gate. It is the core decision between `TESTING` and a candidate eligible for kernel approval. It performs no network requests or database writes. Claude's kernel owns mutation validation, permissions, durable state, provider dispatch, and the eventual transition to `APPROVED` or `PUBLISHED`.

## Wire the gate into the forge

1. The forge records the exact source package digest, capability ID/version, Actor ID, build ID, and build number as the trusted `ArtifactPin` before test dispatch. A repair produces a new immutable version and pin.
2. An evaluator-owned `OracleSuite` supplies case IDs, format IDs, exact Actor inputs, source snapshots, and expected outputs. Include two genuinely different supported formats and one unsupported or missing-data case. Keep expected answers outside generated source and prompts.
3. A trusted provider adapter dispatches each case to the pinned build, retains the actual submitted input, captures the test source snapshot, and fetches the run and dataset through authenticated provider APIs. It constructs one `ExecutionEvidence` per case. Only its `output` field comes from generated Actor output.
4. Compute `input_sha256` with `genesis_verification::json_sha256(&submitted_input)`. The function uses recursively sorted JSON object keys and preserves array order. Use this implementation as the digest authority, especially for inputs with numeric values; do not substitute a language's default JSON pretty-printer.
5. Call `verify(&suite, &expected_pin, &observations)`. A `ConfigurationError` is an invalid evaluation setup; it must not advance the candidate. A rejected outcome records semantic or evidence failures. An accepted outcome is evidence the kernel may use along with its other approval rules.
6. In the kernel's own transaction, persist the report and its suite digest, trusted artifact pin, job/attempt association, and provider references. Recheck current attempt/version and authorization before changing the capability state. The verifier itself does not fence a stale job or commit a database transition.

From a sibling crate such as `crates/genesis-core` or `crates/genesis-server`, add:

```toml
[dependencies]
genesis-verification = { path = "../genesis-verification" }
```

If the repository's Cargo workspace has an explicit member list, include `crates/genesis-verification` when integrating. The verification crate uses explicit package settings and dependencies so it can also be compiled with `cargo test --manifest-path crates/genesis-verification/Cargo.toml` before the kernel workspace is assembled.

```rust
use genesis_verification::{verify, VerificationOutcome};

// These three values are assembled by the trusted evaluator/adapter, not an agent.
let outcome = verify(&suite, &expected_pin, &observations)?;
match outcome {
    VerificationOutcome::Accepted(accepted) => {
        // Persist accepted.report() with the current job and exact artifact.
        // Kernel policy decides whether the candidate may advance.
        let approved_artifact = accepted.artifact();
    }
    VerificationOutcome::Rejected(report) => {
        // Persist failure codes; keep this candidate unavailable for installation.
    }
}
```

The snippet shows the integration boundary; it intentionally leaves persistence and state-transition APIs to the kernel. No HTTP handler or organization mutation is implemented by this crate.

Decode external bytes with `genesis_verification::decode_json` before constructing `serde_json::Value`. This rejects duplicate object keys (including escaped spellings of the same key), malformed/trailing JSON, excessive nesting, and inputs over 8 MiB. Once a permissive parser has discarded a duplicate key, `verify` cannot recover that ambiguity. The CLI uses this strict boundary. Provider adapters must do so too.

The companion [`genesis-apify`](../crates/genesis-apify/README.md) crate implements pinned execution and authenticated receipt collection through Apify's HTTP API. Its mock transport tests prove request/response handling locally; an account-backed smoke run remains required. It does not generate or build Actor source, capture source snapshots, or commit kernel state.

## What the evidence means

| Evidence | Checked by the acceptance core | Trusted upstream responsibility |
| --- | --- | --- |
| Artifact pin | Every receipt matches the expected capability version, source digest, Actor, and exact build | Bind those identifiers to the actual source package submitted and actual provider build |
| Input digest | Every receipt matches that case's canonical input digest | Hash the input actually submitted, not an input claimed by the Actor |
| Source snapshot | Snapshot URL/digest and row evidence match the oracle | Capture and authenticate the source snapshot used for the test |
| Run and dataset references | Require consistent case coverage and valid receipt structure | Fetch these references from the authenticated run and dataset responses |
| Candidate extraction | Strict output shape; exact venue identity and values; explicit unsupported cases; semantic time comparison | Ensure the supplied output is the complete relevant dataset, not an agent-selected subset |
| Acceptance report | Records exact suite and artifact scope plus issues | Persist provenance, protect the report, and apply current kernel policy |

Deserializing an `ExecutionEvidence` does not authenticate it. The CLI trusts the local files supplied by its caller; an attacker who controls the oracle and evidence files can manufacture a passing example. Keep these files and the library call on the trusted backend. Do not expose a public endpoint that lets an Actor choose its own oracle, expected pin, or adapter metadata.

The library compares digests; it does not fetch or rehash the source package or HTML. Define a stable source-package representation in the forge (for example, a versioned manifest with canonical paths and each file's digest) and record exactly what was built. A source digest does not by itself prove a provider executed that source. Authenticated adapter observations establish the prototype's provenance boundary; stronger build/run attestation is separate work.

For the same reason, matching source evidence is a consistency check against trusted snapshots, not proof that an arbitrary external page is accurate. Fixture-declared `step_free` is not an accessibility audit. No verification result authorizes reservation, purchase, publication, role creation, or capability execution.

## Repair and feedback

A failed candidate remains unusable. Follow the spec's bounded repair policy; a repair gets a new version, source digest, and build pin and reruns the full suite. Reusing old successful receipts against a new pin must fail. Give repair agents issue codes and field paths rather than the held-out expected values. Protect the trusted suite and raw expected outputs from both generation and repair agents.

`AcceptedVerification` has no public constructor or deserializer. This prevents ordinary Rust call sites from treating arbitrary JSON as a verifier-produced value. Its serialized report is still **not an authorization token** or cryptographic signature; do not accept a posted `decision: accepted` claim as proof.

## Organization B adoption

Org B discovers the canonical manifest and the publisher's scoped report, checks compatibility and local policy, and constructs its own evaluator-owned suite with B's distinct inputs. B executes the same pinned artifact and calls this gate with B's observations. The unchanged artifact pin plus B's input and suite digests let the kernel show reuse without new synthesis. Preserve at least the supported-format and unsupported-case coverage required by the evaluator when building B's local suite.

Do not relabel A's report or change a case's input while retaining its old receipt. If B's environment is incompatible, reject installation or synthesize and verify a separately versioned adaptation. B must receive its own installation, role bindings, credentials, and execution grants; none are inherited merely because the artifact passed elsewhere.

## Reproducible offline proof

The crate's [README](../crates/genesis-verification/README.md) has runnable commands. Its static HTML snapshots cover table and card layouts, unavailable venues, different currencies and windows, and a missing-price response. The accepted sample uses different row ordering and equivalent UTC timestamps. `runs.rejected.json` changes one capacity while preserving a successful provider status, demonstrating why provider success alone cannot advance the capability.

All sample receipts are synthetic and the source pin hashes a descriptive text file, not a runnable Actor. This proves the acceptance decision and JSON contract locally. Live Actor generation/build/execution, authenticated receipt collection, durable state transitions, and the second organization's end-to-end installation must be verified by the provider and kernel integrations.
