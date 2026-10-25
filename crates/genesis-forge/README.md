# GENESIS capability forge

The forge also implements experimental development-method source generation,
exact-parent revision, registry-client preparation and stage/chain cost tools.
See [generation](../../docs/invention-lab/contracts/generation.md),
[revision](../../docs/invention-lab/contracts/revision.md) and
[accounting CLI](../../docs/invention-lab/contracts/accounting-cli.md).
The Actor pipeline below is one supported path, not the entire crate.

Rust source packaging and a checkpointed source → Actor → build → independent
verification pipeline. It returns a verified manifest draft to the organization
kernel; it cannot publish, install, grant execution, or create organizational roles.

Read [the integration guide](../../docs/FORGE_INTEGRATION.md) for commands,
recovery rules, the narrow TypeScript parser contract, and validation limitations.

```sh
cargo test --locked --manifest-path crates/genesis-forge/Cargo.toml
cargo run --locked --manifest-path crates/genesis-forge/Cargo.toml -- --help
```
