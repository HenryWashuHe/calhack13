# genesis-fetch

Rust-owned discovery, recruitment contracts, message evidence and a private bridge
client. A discovered specialist is only a candidate. A native accepted contract is
not a kernel role, and a collected task result is not semantic verification.

See [the runtime guide](../../docs/FETCH_RUNTIME.md), the
[signed Python transport](../../services/fetch-bridge/README.md), and
[the executable two-organization demo](../../scripts/demo-fetch-recruitment.py).

```sh
cargo test --locked -p genesis-fetch
cargo build --locked -p genesis-fetch
uv run --project services/fetch-bridge --frozen python scripts/demo-fetch-recruitment.py
```

The demo uses three separate processes and real native SDK signed HTTP messages,
but synthetic advisory work and explicit loopback peers. It does not register
public agents, exercise authenticated Agentverse discovery or activate kernel roles.
