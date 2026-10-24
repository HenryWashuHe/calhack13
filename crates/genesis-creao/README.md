# genesis-creao

Rust CREAO Account API client and durable specialist operations. Agent creation
returns a native run ID before the agent exists. Persist that receipt and observe
it using GETs. Native completion never establishes readiness, domain verification,
or organization authority.

The crate provides bounded authenticated requests, strict JSON models, private
account/request-bound run tickets, agent inspection, a SQLite operation journal,
and a shared SQLite account limiter. The CLI can prepare, inspect, advance and
recover backend operations. No command accepts a token or provider URL argument.

See [the integration guide](../../docs/CREAO_RUNTIME.md) for commands, kernel hooks,
verified contracts, trust limits and the remaining live-account smoke test.

```sh
cargo test --locked -p genesis-creao -p genesis-providers
cargo clippy --locked -p genesis-creao -p genesis-providers --all-targets -- -D warnings
```

These tests use loopback native-protocol simulations; no cloud account or actual
CREAO execution is claimed. The example spec is trusted demonstration input, not
an organization authorization grant.
