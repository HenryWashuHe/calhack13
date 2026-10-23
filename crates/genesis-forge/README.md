# GENESIS capability forge

Rust source packaging and a checkpointed source → Actor → build → independent
verification pipeline. It returns a verified manifest draft to the organization
kernel; it cannot publish, install, grant execution, or create organizational roles.

Read [the integration guide](../../docs/FORGE_INTEGRATION.md) for commands,
recovery rules, the narrow TypeScript parser contract, and validation limitations.

```sh
cargo test --locked --manifest-path crates/genesis-forge/Cargo.toml
cargo run --locked --manifest-path crates/genesis-forge/Cargo.toml -- --help
```
