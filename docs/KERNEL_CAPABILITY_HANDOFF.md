# Kernel and capability integration checkpoint

Checked October 21, 2026 against kernel branch `feat/kernel-ui` at `5c6fdc2` and
capability branch `codex/capability-forge` at `65cc594`. Claude still owns kernel/UI;
this record proposes integration changes without changing those paths.

## Workspace coexistence verified

An isolated temporary tree combined both branches' source and added these members
to the kernel's root `Cargo.toml`:

```toml
"crates/genesis-verification",
"crates/genesis-apify",
"crates/genesis-forge",
```

After regenerating the temporary root `Cargo.lock`, `cargo test --workspace
--locked` passed **181 tests and one documentation example**. No dependency
conflicts were found. This was a compile/test integration check, not a production
wiring of the provider interfaces. No tracked kernel files were edited.

When the kernel branch merges, its owner should add these members and regenerate
the root lockfile as part of the merge. Cargo will then use that root lockfile;
the per-crate lockfiles currently support the standalone capability branch.
The combined project needs Rust 1.94+ for the capability crates and Node.js 24+
for the Actor runtime.

Later observed checkpoint `21103d1` includes Claude's handoff and workspace change
`4383315`, which already adds `genesis-apify` and `genesis-verification` after merging
main. Only `genesis-forge` remains to be added when PR #2 is integrated. The kernel
implementation itself is unchanged from the tested source checkpoint above.

## Wire the complete execution identity

The current `CapabilityExecutor::submit(capability, actor_id, build_number,
input)` loses fields required by the real adapter. The adapter already accepts
`genesis_apify::RunRequest`, which contains:

- `case_id` for the backend's case/correlation identity;
- complete `ArtifactPin`: capability ID/version, source digest, Actor ID, build ID
  and exact build number;
- input and expected `SourceSnapshot` from trusted backend configuration.

Carry those fields through the kernel's request or an equivalent kernel DTO.
The source snapshot is required for deterministic acceptance runs. A production
discovery run against a changing page needs a separate freshness/capture policy;
do not invent a digest merely to satisfy this interface.

The kernel persists submission intent before the POST, then the returned typed
`RunTicket` before polling. Add a resume/collect path that consumes the original
request and ticket and returns `ExecutionEvidence`. That path makes GET requests
only. Lost POST responses remain `Unknown`/reconciliation; the absence of a saved
ticket does not authorize resubmission. Runtime string references alone do not
replace the typed ticket's request fingerprint checks.

Adapter errors expose sanitized kind, stage, known safe IDs and an ambiguity flag.
Do not put raw provider bodies into `ProviderError::Http.body`: its current Display
prints that field. Tokens and provider error echoes must not reach logs or UI.

## Bridge verification without trusting a passed flag

The kernel's `VerificationReport { passed, cases, ... }` is a presentation/domain
DTO. The verification crate computes an independent report from a trusted oracle,
an exact artifact pin and complete execution receipts. Only after that computation
should the adapter map the result into the kernel DTO.

For a newly forged artifact, restore `ForgeSession`, validate it against the
original suite, then call `verified(&suite)`. For Org B, execute fresh local cases
against the inherited pin and call `genesis_verification::verify` on B's receipts.
Never manufacture `AcceptedVerification` from incoming JSON, promote a saved
`passed: true`, or count DevStub receipts as Apify evidence. Keep the full report
and receipts in trusted backend storage and expose a reference to them in the
kernel's event/manifest layer.

The forge returns a draft. The kernel still creates the role, enforces budgets,
publishes the approved package, checks B's compatibility, authorizes its installation
and fences retired assignments. The local reuse demo proves parser reuse and input
binding; it does not establish those organization lifecycle transitions.

## Two feature-unification findings for the kernel owner

The optional stress command `cargo test --workspace --locked --features
serde_json/preserve_order --no-fail-fast` passed **180 tests and one documentation
example**, with one failure in `crates/genesis-core/tests/schema_drift.rs:14`.
The expected/generated schemas parse to identical JSON values; the test compares
pretty-printed bytes whose object-key order changes. Compare parsed values (or
canonicalized values) so this test detects schema changes rather than formatting.

A separate public-API probe found that
`genesis_providers::ProvisionRequest::config_digest()` also changes for the same
request under this feature because it hashes `serde_json::Value::to_string()`.
Use the canonical sorted-object digest helper, or an equivalent stable serializer,
for persisted configuration identities. `genesis_verification::json_sha256` stayed
identical in both profiles. Add a feature-unification regression before relying on
that digest for reconciliation across builds.

This optional feature is not enabled by the current kernel dependency set; the
normal combined workspace passes. These findings are recorded separately from
the passing capability suite so they are not mistaken for a current kernel outage.

## Remaining live gates

The forge API, Actor SDK runtime, durable CLI and independent verifier have local
tests and a reproducible cross-language demo. Account-backed Actor creation/builds,
runtime source synthesis, Elastic publication/discovery, Fetch specialist execution,
CREAO continuing responsibility and full kernel/UI integration still need live
evidence. No local simulation or serialized report substitutes for that evidence.
