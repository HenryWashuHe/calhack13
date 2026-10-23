# GENESIS capability acceptance core

This Rust library makes the semantic acceptance decision between a completed capability run and an approved capability version. It compares untrusted venue extraction output with an independently authored oracle and requires evidence for the exact source, Actor, build, input, and source snapshot. The kernel still owns publication, installation, authority, and organization changes.

## Run the offline example

From the repository root, with Rust 1.94 or newer and Cargo available:

```sh
cargo test --manifest-path crates/genesis-verification/Cargo.toml

cargo run --manifest-path crates/genesis-verification/Cargo.toml --bin genesis-verify -- \
  --suite crates/genesis-verification/examples/fixtures/trusted/suite.json \
  --artifact crates/genesis-verification/examples/fixtures/artifact.json \
  --runs crates/genesis-verification/examples/fixtures/runs.json
```

The successful invocation prints a JSON report and exits `0`. Repeat it with `runs.rejected.json` to see an intentionally incorrect capacity rejected with exit `1`. Invalid arguments, unreadable or malformed files, and invalid trusted configuration exit `2`.

These examples are entirely offline. `synthetic-artifact.txt` is a descriptor, not an executable Actor; its bytes supply a reproducible source digest. Actor, build, run, and dataset IDs are synthetic. The `.example` URLs name fixture snapshots and are not deployed endpoints. No example receipt proves an Apify execution happened.

## Fixture boundaries

| Path under `examples/fixtures` | Purpose |
| --- | --- |
| `portal/table.html` | Supported table with available and unavailable venues |
| `portal/cards.html` | Independently structured cards with different currency, capacities, and time windows |
| `portal/missing-data.html` | A listing without a numeric price; the correct result is explicitly unsupported |
| `trusted/suite.json` | Trusted expected answers, separate from the portal served to an Actor |
| `artifact.json` | Expected synthetic source and build pin |
| `runs.json` | Separately authored synthetic adapter observations and candidate output |
| `runs.rejected.json` | Same observations with one incorrect capacity |

Every snapshot digest hashes the exact HTML file bytes. Candidate output deliberately changes row ordering and expresses times in UTC; both preserve the expected meaning. The verifier checks all listed venues, including unavailable ones. It does not filter venues to the requested attendee count or make a booking decision.

The checked-in oracle makes this a reproducible example, **not a held-out evaluation of an agent with repository access**. For generation tests, prepare evaluator-owned variants, provide only the allowed portal content to the generator, and never copy the trusted suite into generated source, prompts, repair feedback, or a public static directory.

## Call from Rust

Add a path dependency from another crate in `crates/`:

```toml
[dependencies]
genesis-verification = { path = "../genesis-verification" }
```

```rust
use genesis_verification::{
    verify, ArtifactPin, ConfigurationError, ExecutionEvidence, OracleSuite,
    VerificationOutcome,
};

fn assess_candidate(
    oracle: &OracleSuite,
    expected_pin: &ArtifactPin,
    observations: &[ExecutionEvidence],
) -> Result<VerificationOutcome, ConfigurationError> {
    verify(oracle, expected_pin, observations)
}
```

`VerificationOutcome::Accepted` contains an `AcceptedVerification` created by the verifier. `Rejected` contains a report with machine-readable issue codes and field paths. The report omits oracle answer values. Use `outcome.report()` to persist the report and `outcome.is_accepted()` for presentation; match the enum when applying kernel state transitions.

Acceptance is scoped to the suite. It is not a grant, a signed attestation, evidence of arbitrary-code containment, or a guarantee of future extraction correctness. See [kernel integration](../../docs/VERIFICATION_INTEGRATION.md) for the adapter trust boundary and cross-organization adoption.
